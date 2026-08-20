use crate::cli::DatabricksCli;
use crate::fetchers::preview::run_sql;
use crate::fetchers::spark_live;
use crate::shape::TableData;

/// Lookback window for the trend/attribution queries — deep enough to
/// beat the Runs API's realistic ~8-20 runs, cheap enough for one query.
pub const WINDOW_DAYS: i64 = 30;

/// Whole-percent success rate, except a rate that rounds up to a clean
/// 100% while genuinely being just short of it (e.g. 727/729 = 99.73%)
/// gets one decimal place instead — showing "100% success" right next
/// to a nonzero failed count reads as contradictory even though the
/// underlying number was never wrong, just rounded past the point that
/// mattered.
pub fn fmt_success_rate(rate: f64) -> String {
    if rate < 100.0 && rate.round() >= 100.0 {
        format!("{rate:.1}%")
    } else {
        format!("{rate:.0}%")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DayOutcome {
    pub date: String,
    pub success: u32,
    pub failed: u32,
    pub other: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskFailure {
    pub task_key: String,
    pub failures: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeFamily {
    Memory,
    Compute,
    General,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputePressure {
    pub avg_cpu_busy_pct: f64,
    pub avg_cpu_wait_pct: f64,
    pub avg_mem_used_pct: f64,
    pub p90_mem_used_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlagSeverity {
    Info,
    Warn,
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HealthFlag {
    pub severity: FlagSeverity,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct JobHealthData {
    pub window_days: i64,
    /// One bar per calendar day the runs backbone covers.
    pub days: Vec<DayOutcome>,
    /// success / (success + failed) as a percentage; "other" outcomes
    /// (canceled, skipped) are excluded from the denominator.
    pub success_rate: f64,
    pub total_runs: u32,
    pub failed_runs: u32,
    /// (days_ago, duration_s), oldest first.
    pub duration_points: Vec<(i64, i64)>,
    /// Recent-third vs older-third average duration, as a percentage
    /// change. Positive = slower. None with too few runs to compare.
    pub duration_trend_pct: Option<f64>,
    /// Failing tasks, most failures first. Empty when unreadable or
    /// when nothing failed.
    pub task_failures: Vec<TaskFailure>,
    /// False when job_task_run_timeline was unreadable — distinct from
    /// "readable but nothing failed", which leaves task_failures empty too.
    pub task_attribution_available: bool,
    /// None when system.compute is unreadable or the join found nothing.
    pub compute: Option<ComputePressure>,
    pub node_family: Option<NodeFamily>,
    pub flags: Vec<HealthFlag>,
    /// True when usage is filtered to the current workspace.
    pub scoped: bool,
}

/// `AND job_id = '<id>'` — quotes stripped rather than escaped, as
/// elsewhere in this codebase's system-table queries.
fn job_clause(job_id: &str) -> String {
    format!(" AND job_id = '{}'", job_id.replace('\'', ""))
}

fn ws_clause(workspace_id: Option<&str>) -> String {
    match workspace_id {
        Some(id) => format!(" AND workspace_id = '{}'", id.replace('\'', "")),
        None => String::new(),
    }
}

/// A run can be sliced across multiple rows sharing one `run_id` — runs
/// longer than an hour get one row per hour of wall-clock time — and
/// `result_state` is populated only on the row that closes the run, NULL
/// on every earlier slice (and on a run still in progress). Grouping by
/// `run_id` and taking MAX(result_state) collapses the slices back into
/// one row per run and picks up that single non-null value; MAX ignores
/// NULLs, so this works whether the run was sliced or not.
fn runs_query(job_id: &str, ws: &str) -> String {
    let jc = job_clause(job_id);
    format!(
        "WITH runs AS ( \
           SELECT run_id, \
                  MIN(period_start_time) AS run_start, \
                  MAX(period_end_time) AS run_end, \
                  MAX(result_state) AS result_state, \
                  MAX(run_duration_seconds) AS run_duration_seconds \
           FROM system.lakeflow.job_run_timeline \
           WHERE period_start_time >= date_sub(current_timestamp(), {WINDOW_DAYS}){jc}{ws} \
           GROUP BY run_id \
         ) \
         SELECT CAST(datediff(current_timestamp(), run_start) AS INT) AS days_ago, \
                CAST(run_start AS DATE) AS date, \
                COALESCE(result_state, 'RUNNING') AS result_state, \
                COALESCE(run_duration_seconds, CAST(run_end AS LONG) - CAST(run_start AS LONG)) AS duration_s \
         FROM runs \
         ORDER BY run_start"
    )
}

/// Same per-row-is-a-slice-not-a-task-run caveat as `runs_query`, keyed
/// by (run_id, task_key) instead of just run_id.
fn task_failures_query(job_id: &str, ws: &str) -> String {
    let jc = job_clause(job_id);
    format!(
        "WITH tasks AS ( \
           SELECT run_id, task_key, MAX(result_state) AS result_state \
           FROM system.lakeflow.job_task_run_timeline \
           WHERE period_start_time >= date_sub(current_timestamp(), {WINDOW_DAYS}){jc}{ws} \
           GROUP BY run_id, task_key \
         ) \
         SELECT task_key, COALESCE(result_state, 'RUNNING') AS result_state, COUNT(*) AS n \
         FROM tasks \
         GROUP BY 1, 2 ORDER BY 3 DESC"
    )
}

/// Joins the job's own clusters to their node-level utilization over the
/// same window. `compute_ids` on `job_run_timeline` is documented as
/// populated only for `WORKFLOW_RUN` run types — an ordinary job's
/// per-run compute lives on `job_task_run_timeline` instead, so that's
/// what this explodes.
fn compute_query(job_id: &str, ws: &str) -> String {
    let jc = job_clause(job_id);
    format!(
        "WITH task_runs AS ( \
           SELECT period_start_time, period_end_time, explode(compute_ids) AS cluster_id \
           FROM system.lakeflow.job_task_run_timeline \
           WHERE period_start_time >= date_sub(current_timestamp(), {WINDOW_DAYS}){jc}{ws} \
         ) \
         SELECT ROUND(AVG(n.cpu_user_percent + n.cpu_system_percent), 1) AS cpu_busy, \
                ROUND(AVG(n.cpu_wait_percent), 1) AS cpu_wait, \
                ROUND(AVG(n.mem_used_percent), 1) AS mem_avg, \
                ROUND(percentile_approx(n.mem_used_percent, 0.9), 1) AS mem_p90 \
         FROM task_runs r \
         JOIN system.compute.node_timeline n \
           ON n.cluster_id = r.cluster_id \
           AND n.start_time < r.period_end_time \
           AND n.end_time > r.period_start_time"
    )
}

/// Current node types of the job's most recent cluster, for the
/// node-family heuristic. `system.compute.clusters` is a slow-changing
/// dimension, so the latest version is picked explicitly. Same
/// task-level `compute_ids` source as `compute_query`.
fn node_family_query(job_id: &str, ws: &str) -> String {
    let jc = job_clause(job_id);
    format!(
        "WITH latest AS ( \
           SELECT explode(compute_ids) AS cluster_id \
           FROM system.lakeflow.job_task_run_timeline \
           WHERE 1=1{jc}{ws} \
           ORDER BY period_start_time DESC LIMIT 1 \
         ) \
         SELECT c.worker_node_type, c.driver_node_type \
         FROM latest l JOIN system.compute.clusters c ON c.cluster_id = l.cluster_id \
         ORDER BY c.change_time DESC LIMIT 1"
    )
}

/// `system.lakeflow.*` uses its own result_state vocabulary, distinct
/// from (and confusingly close to) the Jobs REST API's — confirmed
/// against Databricks' documented "Result state values" for this table:
/// SUCCEEDED, FAILED, SKIPPED, CANCELLED, TIMED_OUT, ERROR, BLOCKED, and
/// NULL for an intermediate/still-running slice. SUCCESS is kept as a
/// defensive fallback in case another workspace or table version uses
/// the REST API's spelling instead.
fn classify(result_state: &str) -> &'static str {
    match result_state.to_uppercase().as_str() {
        "SUCCEEDED" | "SUCCESS" => "success",
        "FAILED" | "ERROR" | "TIMED_OUT" => "failed",
        // SKIPPED, CANCELLED, BLOCKED, RUNNING (our own NULL placeholder)
        // aren't a pass/fail signal — excluded from the rate, not double
        // counted as either.
        _ => "other",
    }
}

/// One raw run row: (days_ago, date, result_state, duration_s).
type RunRow = (i64, String, String, i64);

fn parse_runs(table: &TableData) -> Vec<RunRow> {
    table
        .rows
        .iter()
        .filter_map(|row| {
            let (days_ago, date, state, dur) = match row.as_slice() {
                [a, d, s, dur] => (a, d, s, dur),
                _ => return None,
            };
            Some((
                days_ago.parse().ok()?,
                date.clone(),
                state.clone(),
                dur.parse().unwrap_or(0),
            ))
        })
        .collect()
}

/// Recent-third vs older-third average duration, as a percentage
/// change. None when there aren't at least two comparable thirds.
fn duration_trend(points: &[(i64, i64)]) -> Option<f64> {
    if points.len() < 6 {
        return None;
    }
    let mut sorted = points.to_vec();
    // Oldest (largest days_ago) first.
    sorted.sort_by_key(|(days_ago, _)| std::cmp::Reverse(*days_ago));
    let n = sorted.len();
    let third = n / 3;
    if third == 0 {
        return None;
    }
    let avg = |slice: &[(i64, i64)]| -> f64 {
        slice.iter().map(|(_, d)| *d as f64).sum::<f64>() / slice.len() as f64
    };
    let older = avg(&sorted[..third]);
    let recent = avg(&sorted[n - third..]);
    if older <= 0.0 {
        return None;
    }
    Some((recent - older) / older * 100.0)
}

fn aggregate_runs(rows: &[RunRow]) -> JobHealthData {
    let mut days: Vec<DayOutcome> = Vec::new();
    let mut duration_points: Vec<(i64, i64)> = Vec::new();
    let (mut success, mut failed, mut other) = (0u32, 0u32, 0u32);

    for (days_ago, date, state, dur) in rows {
        if days.last().map(|d| &d.date) != Some(date) {
            days.push(DayOutcome {
                date: date.clone(),
                success: 0,
                failed: 0,
                other: 0,
            });
        }
        let day = days.last_mut().unwrap();
        match classify(state) {
            "success" => {
                day.success += 1;
                success += 1;
            }
            "failed" => {
                day.failed += 1;
                failed += 1;
            }
            _ => {
                day.other += 1;
                other += 1;
            }
        }
        duration_points.push((*days_ago, *dur));
    }

    let denom = success + failed;
    let success_rate = if denom > 0 {
        success as f64 / denom as f64 * 100.0
    } else {
        0.0
    };

    JobHealthData {
        window_days: WINDOW_DAYS,
        days,
        success_rate,
        total_runs: success + failed + other,
        failed_runs: failed,
        duration_trend_pct: duration_trend(&duration_points),
        duration_points,
        task_failures: Vec::new(),
        task_attribution_available: false,
        compute: None,
        node_family: None,
        flags: Vec::new(),
        scoped: false,
    }
}

fn parse_task_failures(table: &TableData) -> Vec<TaskFailure> {
    let mut by_task: Vec<(String, u32, u32)> = Vec::new(); // (key, failures, total)
    for row in &table.rows {
        let (key, state, n) = match row.as_slice() {
            [k, s, n] => (k, s, n),
            _ => continue,
        };
        let n: u32 = n.parse().unwrap_or(0);
        let entry = match by_task.iter_mut().find(|(k, _, _)| k == key) {
            Some(e) => e,
            None => {
                by_task.push((key.clone(), 0, 0));
                by_task.last_mut().unwrap()
            }
        };
        entry.2 += n;
        if classify(state) == "failed" {
            entry.1 += n;
        }
    }
    let mut out: Vec<TaskFailure> = by_task
        .into_iter()
        .filter(|(_, failures, _)| *failures > 0)
        .map(|(task_key, failures, total)| TaskFailure {
            task_key,
            failures,
            total,
        })
        .collect();
    out.sort_by(|a, b| {
        b.failures
            .cmp(&a.failures)
            .then_with(|| a.task_key.cmp(&b.task_key))
    });
    out
}

fn parse_compute(table: &TableData) -> Option<ComputePressure> {
    let row = table.rows.first()?;
    match row.as_slice() {
        [busy, wait, avg, p90] => Some(ComputePressure {
            avg_cpu_busy_pct: busy.parse().ok()?,
            avg_cpu_wait_pct: wait.parse().ok()?,
            avg_mem_used_pct: avg.parse().ok()?,
            p90_mem_used_pct: p90.parse().ok()?,
        }),
        _ => None,
    }
}

/// Coarse memory/compute/general classification from a cloud node-type
/// name, e.g. "r5.xlarge" (AWS), "Standard_E8s_v5" (Azure),
/// "n2-highmem-4" (GCP). Approximate by design — good enough to flag an
/// obvious mismatch, not a substitute for a real instance-type catalog.
fn classify_node_family(node_type: &str) -> NodeFamily {
    let s = node_type.to_lowercase();

    if s.contains("highmem") {
        return NodeFamily::Memory;
    }
    if s.contains("highcpu") {
        return NodeFamily::Compute;
    }
    if let Some(rest) = s.strip_prefix("standard_") {
        return match rest.chars().next() {
            Some('e') => NodeFamily::Memory,
            Some('f') => NodeFamily::Compute,
            Some('d') | Some('a') | Some('b') => NodeFamily::General,
            _ => NodeFamily::Unknown,
        };
    }
    if s.contains("-standard-") {
        return NodeFamily::General;
    }
    let prefix: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    match prefix.as_str() {
        "r" => NodeFamily::Memory,
        "c" => NodeFamily::Compute,
        "m" => NodeFamily::General,
        _ => NodeFamily::Unknown,
    }
}

fn parse_node_family(table: &TableData) -> Option<NodeFamily> {
    let row = table.rows.first()?;
    let node_type = match row.as_slice() {
        [worker, driver] if !worker.is_empty() && worker != "␀" => worker,
        [_, driver] => driver,
        _ => return None,
    };
    if node_type.is_empty() || node_type == "␀" {
        return None;
    }
    Some(classify_node_family(node_type))
}

const MIN_RUN_SAMPLE: u32 = 5;
const SUCCESS_RATE_CRITICAL: f64 = 80.0;
const SUCCESS_RATE_WARN: f64 = 95.0;
const DURATION_SLOWDOWN_WARN: f64 = 30.0;
const DURATION_SLOWDOWN_CRITICAL: f64 = 75.0;
const MEM_P90_CRITICAL: f64 = 90.0;
const MEM_AVG_WARN: f64 = 80.0;
const CPU_BUSY_OVERPROVISIONED: f64 = 30.0;
const CPU_WAIT_WARN: f64 = 20.0;
const MIN_FAILURES_FOR_ATTRIBUTION: u32 = 3;
const DOMINANT_FAILURE_FRACTION: f64 = 0.8;

/// Threshold-triggered one-liners built directly from signals already in
/// `data` — no tuning advice beyond what those signals directly support.
fn derive_flags(data: &JobHealthData) -> Vec<HealthFlag> {
    let mut flags = Vec::new();

    if data.total_runs >= MIN_RUN_SAMPLE {
        if data.success_rate < SUCCESS_RATE_CRITICAL {
            flags.push(HealthFlag {
                severity: FlagSeverity::Critical,
                message: format!(
                    "success rate is only {:.0}% over the last {} days ({} of {} runs failed)",
                    data.success_rate, data.window_days, data.failed_runs, data.total_runs
                ),
            });
        } else if data.success_rate < SUCCESS_RATE_WARN {
            flags.push(HealthFlag {
                severity: FlagSeverity::Warn,
                message: format!(
                    "success rate is {:.0}% over the last {} days — worth a look",
                    data.success_rate, data.window_days
                ),
            });
        }
    }

    if let Some(pct) = data.duration_trend_pct {
        if pct > DURATION_SLOWDOWN_CRITICAL {
            flags.push(HealthFlag {
                severity: FlagSeverity::Critical,
                message: format!(
                    "runs have gotten {pct:.0}% slower recently vs earlier in the window — \
                     check for data growth or a regression"
                ),
            });
        } else if pct > DURATION_SLOWDOWN_WARN {
            flags.push(HealthFlag {
                severity: FlagSeverity::Warn,
                message: format!(
                    "runs have gotten {pct:.0}% slower recently vs earlier in the window"
                ),
            });
        }
    }

    if let Some(c) = &data.compute {
        if c.p90_mem_used_pct > MEM_P90_CRITICAL {
            flags.push(HealthFlag {
                severity: FlagSeverity::Critical,
                message: format!(
                    "p90 memory utilization is {:.0}% — spill risk is high; consider a \
                     memory-optimized node type or more workers",
                    c.p90_mem_used_pct
                ),
            });
        } else if c.avg_mem_used_pct > MEM_AVG_WARN {
            flags.push(HealthFlag {
                severity: FlagSeverity::Warn,
                message: format!(
                    "memory runs hot (avg {:.0}%) — headroom is thin",
                    c.avg_mem_used_pct
                ),
            });
        }

        if c.avg_cpu_busy_pct < CPU_BUSY_OVERPROVISIONED {
            flags.push(HealthFlag {
                severity: FlagSeverity::Info,
                message: format!(
                    "average CPU utilization is only {:.0}% — this job's cluster looks \
                     over-provisioned",
                    c.avg_cpu_busy_pct
                ),
            });
        }

        if c.avg_cpu_wait_pct > CPU_WAIT_WARN {
            flags.push(HealthFlag {
                severity: FlagSeverity::Warn,
                message: format!(
                    "CPU wait is high (avg {:.0}%) — likely I/O-bound; faster storage/network \
                     may help more than more CPU",
                    c.avg_cpu_wait_pct
                ),
            });
        }

        if let Some(family) = data.node_family {
            if c.p90_mem_used_pct > MEM_P90_CRITICAL
                && family != NodeFamily::Memory
                && family != NodeFamily::Unknown
            {
                flags.push(HealthFlag {
                    severity: FlagSeverity::Info,
                    message: "high memory pressure on a non-memory-optimized node type — a \
                        memory-optimized family may fit better"
                        .to_string(),
                });
            }
        }
    }

    if let Some(top) = data.task_failures.first() {
        let total_failures: u32 = data.task_failures.iter().map(|t| t.failures).sum();
        if top.failures >= MIN_FAILURES_FOR_ATTRIBUTION && total_failures > 0 {
            let fraction = top.failures as f64 / total_failures as f64;
            if fraction >= DOMINANT_FAILURE_FRACTION {
                flags.push(HealthFlag {
                    severity: FlagSeverity::Info,
                    message: format!(
                        "`{}` accounts for {:.0}% of task failures — start debugging there",
                        top.task_key,
                        fraction * 100.0
                    ),
                });
            }
        }
    }

    flags
}

const SPILL_SKEW_RATIO: f64 = 3.0;

/// Flags that need both the system-table compute pressure and the live/
/// event-log stage data to say something sharper than either signal
/// could alone. Computed at render time — not stored on `JobHealthData`
/// — since the two fetches resolve independently of each other.
pub fn cross_signal_flags(
    data: &JobHealthData,
    live: &spark_live::SparkLiveData,
) -> Vec<HealthFlag> {
    let mut flags = Vec::new();

    if let Some(worst) = live
        .stages
        .iter()
        .filter(|s| s.memory_bytes_spilled + s.disk_bytes_spilled > 0)
        .max_by_key(|s| s.memory_bytes_spilled + s.disk_bytes_spilled)
    {
        let confirmed_by_pressure = data.compute.as_ref().is_some_and(|c| {
            c.p90_mem_used_pct > MEM_P90_CRITICAL || c.avg_mem_used_pct > MEM_AVG_WARN
        });
        let message = if confirmed_by_pressure {
            format!(
                "`{}` spilled to disk in the most recent run — matches the sustained memory \
                 pressure above; a memory-optimized node type or more workers would likely help",
                worst.name
            )
        } else {
            format!(
                "`{}` spilled to disk in the most recent run — worth watching for memory \
                 pressure even without a longer-term signal to confirm it",
                worst.name
            )
        };
        flags.push(HealthFlag {
            severity: if confirmed_by_pressure {
                FlagSeverity::Critical
            } else {
                FlagSeverity::Warn
            },
            message,
        });
    }

    if let Some(worst) = live
        .stages
        .iter()
        .filter(|s| s.skew_ratio >= SPILL_SKEW_RATIO)
        .max_by(|a, b| {
            a.skew_ratio
                .partial_cmp(&b.skew_ratio)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        flags.push(HealthFlag {
            severity: FlagSeverity::Warn,
            message: format!(
                "`{}` shows {:.1}x task skew (slowest task {}ms vs median {}ms) — that's a \
                 data/partitioning issue, not something more compute fixes; look for a skewed \
                 join or group-by key, try salting or repartitioning, or check whether adaptive \
                 query execution's skew-join handling is on",
                worst.name,
                worst.skew_ratio,
                worst.max_task_duration_ms,
                worst.median_task_duration_ms
            ),
        });
    }

    flags
}

/// Deep per-job diagnostics: success-rate and duration trend over
/// `WINDOW_DAYS`, per-task failure attribution, CPU/memory pressure from
/// the job's own clusters, and heuristic flags built from those signals.
/// Only the runs backbone (system.lakeflow.job_run_timeline) is
/// must-succeed; task attribution and compute pressure are bonuses that
/// degrade independently when their schemas aren't readable.
pub async fn fetch(
    cli: &DatabricksCli,
    warehouse_id: &str,
    job_id: &str,
    workspace_id: Option<&str>,
) -> Result<JobHealthData, String> {
    let ws = ws_clause(workspace_id);

    let runs_table = run_sql(cli, &runs_query(job_id, &ws), warehouse_id).await?;
    let mut data = aggregate_runs(&parse_runs(&runs_table));
    data.scoped = workspace_id.is_some();

    if let Ok(table) = run_sql(cli, &task_failures_query(job_id, &ws), warehouse_id).await {
        data.task_failures = parse_task_failures(&table);
        data.task_attribution_available = true;
    }
    if let Ok(table) = run_sql(cli, &compute_query(job_id, &ws), warehouse_id).await {
        data.compute = parse_compute(&table);
    }
    if let Ok(table) = run_sql(cli, &node_family_query(job_id, &ws), warehouse_id).await {
        data.node_family = parse_node_family(&table);
    }

    data.flags = derive_flags(&data);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(days_ago: i64, date: &str, state: &str, dur: i64) -> RunRow {
        (days_ago, date.to_string(), state.to_string(), dur)
    }

    #[test]
    fn success_rate_formatting_never_hides_a_known_failure_behind_a_clean_100() {
        // 727/729 = 99.726...% — rounds to a misleading "100%" at 0dp.
        assert_eq!(fmt_success_rate(727.0 / 729.0 * 100.0), "99.7%");
        // Genuinely 100% stays clean.
        assert_eq!(fmt_success_rate(100.0), "100%");
        // Ordinary values are unaffected — still whole percent.
        assert_eq!(fmt_success_rate(87.4), "87%");
        assert_eq!(fmt_success_rate(0.0), "0%");
    }

    #[test]
    fn same_day_rows_are_summed_into_one_bar() {
        let rows = vec![
            row(2, "2026-08-01", "SUCCESS", 100),
            row(2, "2026-08-01", "FAILED", 50),
            row(1, "2026-08-02", "SUCCESS", 120),
            row(0, "2026-08-03", "SUCCESS", 110),
        ];
        let data = aggregate_runs(&rows);
        assert_eq!(data.days.len(), 3);
        assert_eq!(data.days[0].success, 1);
        assert_eq!(data.days[0].failed, 1);
        assert_eq!(data.total_runs, 4);
        assert_eq!(data.failed_runs, 1);
        assert_eq!(data.success_rate, 75.0);
    }

    #[test]
    fn other_outcomes_are_excluded_from_the_success_rate_denominator() {
        let rows = vec![
            row(2, "2026-08-01", "SUCCESS", 100),
            row(1, "2026-08-02", "CANCELED", 0),
            row(0, "2026-08-03", "SKIPPED", 0),
        ];
        let data = aggregate_runs(&rows);
        assert_eq!(data.total_runs, 3);
        assert_eq!(data.success_rate, 100.0);
    }

    #[test]
    fn succeeded_is_recognized_as_the_lakeflow_vocabulary_for_success() {
        // system.lakeflow.job_run_timeline uses SUCCEEDED/ERROR, not the
        // Jobs REST API's SUCCESS/FAILED — both must count correctly.
        let rows = vec![
            row(1, "2026-08-01", "SUCCEEDED", 100),
            row(0, "2026-08-02", "ERROR", 50),
        ];
        let data = aggregate_runs(&rows);
        assert_eq!(data.total_runs, 2);
        assert_eq!(data.failed_runs, 1);
        assert_eq!(data.success_rate, 50.0);
    }

    #[test]
    fn duration_trend_is_none_with_too_few_points() {
        let points = vec![(4, 100), (3, 100), (2, 100), (1, 100), (0, 100)];
        assert_eq!(duration_trend(&points), None);
    }

    #[test]
    fn duration_trend_detects_a_slowdown() {
        let points = vec![(8, 100), (7, 100), (5, 150), (4, 150), (1, 200), (0, 200)];
        let pct = duration_trend(&points).unwrap();
        assert!(pct > 0.0, "expected a slowdown, got {pct}");
    }

    #[test]
    fn duration_trend_detects_a_speedup() {
        let points = vec![(8, 200), (7, 200), (6, 200), (2, 100), (1, 100), (0, 100)];
        let pct = duration_trend(&points).unwrap();
        assert!(pct < 0.0, "expected a speedup, got {pct}");
    }

    #[test]
    fn task_failures_sort_descending_with_alphabetic_tiebreak() {
        let table = TableData {
            headers: vec!["task_key".into(), "result_state".into(), "n".into()],
            rows: vec![
                vec!["extract".into(), "SUCCESS".into(), "10".into()],
                vec!["extract".into(), "FAILED".into(), "2".into()],
                vec!["load".into(), "FAILED".into(), "5".into()],
                vec!["transform".into(), "FAILED".into(), "5".into()],
            ],
        };
        let failures = parse_task_failures(&table);
        assert_eq!(failures.len(), 3);
        assert_eq!(failures[0].task_key, "load");
        assert_eq!(failures[1].task_key, "transform");
        assert_eq!(failures[2].task_key, "extract");
        assert_eq!(failures[2].total, 12);
    }

    #[test]
    fn classify_node_family_examples() {
        let cases = [
            ("r5.xlarge", NodeFamily::Memory),
            ("r6i.2xlarge", NodeFamily::Memory),
            ("Standard_E8s_v5", NodeFamily::Memory),
            ("n2-highmem-4", NodeFamily::Memory),
            ("c5.4xlarge", NodeFamily::Compute),
            ("Standard_F8s_v2", NodeFamily::Compute),
            ("n2-highcpu-8", NodeFamily::Compute),
            ("m5.xlarge", NodeFamily::General),
            ("Standard_D8s_v5", NodeFamily::General),
            ("n2-standard-4", NodeFamily::General),
            ("totally-unknown-type", NodeFamily::Unknown),
        ];
        for (input, expected) in cases {
            assert_eq!(classify_node_family(input), expected, "input: {input}");
        }
    }

    fn base_data() -> JobHealthData {
        JobHealthData {
            window_days: WINDOW_DAYS,
            days: Vec::new(),
            success_rate: 100.0,
            total_runs: MIN_RUN_SAMPLE,
            failed_runs: 0,
            duration_points: Vec::new(),
            duration_trend_pct: None,
            task_failures: Vec::new(),
            task_attribution_available: false,
            compute: None,
            node_family: None,
            flags: Vec::new(),
            scoped: false,
        }
    }

    #[test]
    fn success_rate_flag_needs_the_minimum_sample() {
        let mut d = base_data();
        d.total_runs = MIN_RUN_SAMPLE - 1;
        d.success_rate = 0.0;
        assert!(derive_flags(&d).is_empty());
    }

    #[test]
    fn success_rate_critical_boundary() {
        let mut d = base_data();
        d.success_rate = SUCCESS_RATE_CRITICAL;
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.severity == FlagSeverity::Critical));

        d.success_rate = SUCCESS_RATE_CRITICAL - 0.1;
        assert!(derive_flags(&d)
            .iter()
            .any(|f| f.severity == FlagSeverity::Critical));
    }

    #[test]
    fn duration_slowdown_boundary() {
        let mut d = base_data();
        d.duration_trend_pct = Some(DURATION_SLOWDOWN_WARN);
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("slower")));

        d.duration_trend_pct = Some(DURATION_SLOWDOWN_WARN + 0.1);
        assert!(derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("slower") && f.severity == FlagSeverity::Warn));
    }

    #[test]
    fn memory_pressure_critical_boundary() {
        let mut d = base_data();
        d.compute = Some(ComputePressure {
            avg_cpu_busy_pct: 50.0,
            avg_cpu_wait_pct: 0.0,
            avg_mem_used_pct: 50.0,
            p90_mem_used_pct: MEM_P90_CRITICAL,
        });
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.severity == FlagSeverity::Critical));

        d.compute.as_mut().unwrap().p90_mem_used_pct = MEM_P90_CRITICAL + 0.1;
        assert!(derive_flags(&d)
            .iter()
            .any(|f| f.severity == FlagSeverity::Critical));
    }

    #[test]
    fn overprovisioned_boundary() {
        let mut d = base_data();
        d.compute = Some(ComputePressure {
            avg_cpu_busy_pct: CPU_BUSY_OVERPROVISIONED,
            avg_cpu_wait_pct: 0.0,
            avg_mem_used_pct: 10.0,
            p90_mem_used_pct: 10.0,
        });
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("over-provisioned")));

        d.compute.as_mut().unwrap().avg_cpu_busy_pct = CPU_BUSY_OVERPROVISIONED - 0.1;
        assert!(derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("over-provisioned")));
    }

    #[test]
    fn cpu_wait_boundary() {
        let mut d = base_data();
        d.compute = Some(ComputePressure {
            avg_cpu_busy_pct: 50.0,
            avg_cpu_wait_pct: CPU_WAIT_WARN,
            avg_mem_used_pct: 10.0,
            p90_mem_used_pct: 10.0,
        });
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("I/O-bound")));

        d.compute.as_mut().unwrap().avg_cpu_wait_pct = CPU_WAIT_WARN + 0.1;
        assert!(derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("I/O-bound")));
    }

    #[test]
    fn node_family_mismatch_needs_high_pressure_and_the_wrong_family() {
        let mut d = base_data();
        d.compute = Some(ComputePressure {
            avg_cpu_busy_pct: 50.0,
            avg_cpu_wait_pct: 0.0,
            avg_mem_used_pct: 50.0,
            p90_mem_used_pct: MEM_P90_CRITICAL + 0.1,
        });
        d.node_family = Some(NodeFamily::Memory);
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("memory-optimized family may fit")));

        d.node_family = Some(NodeFamily::Compute);
        assert!(derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("memory-optimized family may fit")));
    }

    #[test]
    fn dominant_task_failure_needs_count_and_fraction() {
        let mut d = base_data();
        d.task_failures = vec![
            TaskFailure {
                task_key: "a".into(),
                failures: 2,
                total: 2,
            },
            TaskFailure {
                task_key: "b".into(),
                failures: 1,
                total: 1,
            },
        ];
        // Top failure count (2) is below the minimum to attribute anything.
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("start debugging there")));

        d.task_failures = vec![
            TaskFailure {
                task_key: "a".into(),
                failures: 3,
                total: 3,
            },
            TaskFailure {
                task_key: "b".into(),
                failures: 1,
                total: 1,
            },
        ];
        // 3/4 = 75%, below the dominance fraction.
        assert!(!derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("start debugging there")));

        d.task_failures = vec![
            TaskFailure {
                task_key: "a".into(),
                failures: 4,
                total: 4,
            },
            TaskFailure {
                task_key: "b".into(),
                failures: 1,
                total: 1,
            },
        ];
        // 4/5 = 80%, at the dominance fraction.
        assert!(derive_flags(&d)
            .iter()
            .any(|f| f.message.contains("start debugging there")));
    }

    fn stage(
        name: &str,
        skew_ratio: f64,
        mem_spilled: i64,
        disk_spilled: i64,
    ) -> spark_live::StageDiag {
        spark_live::StageDiag {
            stage_id: 1,
            name: name.to_string(),
            num_tasks: 3,
            max_task_duration_ms: 400,
            median_task_duration_ms: 100,
            skew_ratio,
            memory_bytes_spilled: mem_spilled,
            disk_bytes_spilled: disk_spilled,
        }
    }

    fn live_data(stages: Vec<spark_live::StageDiag>) -> spark_live::SparkLiveData {
        spark_live::SparkLiveData {
            app_id: "app-1".to_string(),
            run_id: "1".to_string(),
            source: spark_live::DiagSource::Live,
            stages,
        }
    }

    #[test]
    fn cross_signal_flags_confirms_spill_against_memory_pressure() {
        let mut d = base_data();
        d.compute = Some(ComputePressure {
            avg_cpu_busy_pct: 50.0,
            avg_cpu_wait_pct: 0.0,
            avg_mem_used_pct: 50.0,
            p90_mem_used_pct: MEM_P90_CRITICAL + 1.0,
        });
        let live = live_data(vec![stage("shuffle", 0.0, 1024, 0)]);
        let flags = cross_signal_flags(&d, &live);
        assert!(flags.iter().any(|f| f.severity == FlagSeverity::Critical
            && f.message.contains("matches the sustained memory pressure")));
    }

    #[test]
    fn cross_signal_flags_notes_spill_without_confirming_pressure() {
        let d = base_data(); // compute: None
        let live = live_data(vec![stage("shuffle", 0.0, 1024, 0)]);
        let flags = cross_signal_flags(&d, &live);
        assert!(flags.iter().any(|f| {
            f.severity == FlagSeverity::Warn && f.message.contains("without a longer-term signal")
        }));
    }

    #[test]
    fn cross_signal_flags_ignores_stages_with_no_spill() {
        let d = base_data();
        let live = live_data(vec![stage("shuffle", 0.0, 0, 0)]);
        assert!(!cross_signal_flags(&d, &live)
            .iter()
            .any(|f| f.message.contains("spilled")));
    }

    #[test]
    fn cross_signal_flags_flags_skew_as_a_data_issue_not_compute() {
        let d = base_data();
        let live = live_data(vec![stage("join", SPILL_SKEW_RATIO, 0, 0)]);
        let flags = cross_signal_flags(&d, &live);
        assert!(flags
            .iter()
            .any(|f| f.message.contains("not something more compute fixes")));

        let live_ok = live_data(vec![stage("join", SPILL_SKEW_RATIO - 0.1, 0, 0)]);
        assert!(!cross_signal_flags(&d, &live_ok)
            .iter()
            .any(|f| f.message.contains("skew")));
    }
}
