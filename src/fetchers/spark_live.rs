use crate::cli::DatabricksCli;
use serde_json::Value;
use std::collections::HashMap;

/// Org id `0` and driver UI port `40001` are a community-documented
/// convention for reaching a cluster's own driver through Databricks'
/// driver-proxy path — this is NOT a documented public API and may not
/// hold on every cloud/workspace. Confirmed against a live workspace:
/// this path genuinely requires the driver to be up — a terminated
/// cluster fails with a clean `INVALID_STATE` error (see
/// `friendly_proxy_error`), not a network timeout. There's no
/// eligibility pre-check here regardless, since "is the driver still
/// up" is exactly what the call itself answers — a single attempt that
/// fails cleanly is the intended scope, not a fallback/scanning loop.
/// The Databricks web UI can still show Spark UI for older, terminated
/// runs; that's a separate, still-unidentified mechanism this module
/// doesn't use.
const DRIVER_PROXY_ORG: &str = "0";
const DRIVER_PROXY_PORT: &str = "40001";

#[derive(Debug, Clone, PartialEq)]
pub struct StageDiag {
    pub stage_id: i64,
    pub name: String,
    pub num_tasks: i64,
    pub max_task_duration_ms: i64,
    pub median_task_duration_ms: i64,
    /// max/median task duration; 0.0 when fewer than 2 tasks ran.
    pub skew_ratio: f64,
    pub memory_bytes_spilled: i64,
    pub disk_bytes_spilled: i64,
}

/// Where the stage data came from — surfaced in the UI since the two
/// sources have different freshness/reliability characteristics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiagSource {
    /// The run's own live driver, via the undocumented driver-proxy path.
    Live,
    /// A delivered Spark event log file, read after the driver (and the
    /// proxy path to it) is gone.
    EventLog,
}

#[derive(Debug, Clone)]
pub struct SparkLiveData {
    pub app_id: String,
    pub run_id: String,
    pub source: DiagSource,
    /// Most recent stages, newest first.
    pub stages: Vec<StageDiag>,
}

/// max/median task duration and their ratio, from a stage's raw task
/// durations. 0.0 skew when fewer than 2 tasks — not a misleading ratio.
fn skew_ratio_of(mut durations: Vec<i64>) -> (i64, i64, f64) {
    durations.sort_unstable();
    let max = durations.last().copied().unwrap_or(0);
    let median = durations.get(durations.len() / 2).copied().unwrap_or(0);
    let skew = if durations.len() >= 2 && median > 0 {
        max as f64 / median as f64
    } else {
        0.0
    };
    (max, median, skew)
}

/// How many recent runs to consider — a run can have no cluster to probe
/// for reasons that have nothing to do with what this feature cares
/// about (skipped by a concurrency policy, a condition that wasn't met,
/// disabled, etc.), and the newest run is exactly the one most likely to
/// be one of those. Confirmed against a live workspace that 5 wasn't
/// always enough (a job can have a longer streak of skipped/canceled
/// runs in a row than that). Cheap regardless of depth: each candidate
/// that turns out to have no cluster fails fast (one `get-run` call, no
/// driver/event-log attempt), so scanning further back costs little.
const RECENT_RUNS_TO_SCAN: usize = 25;

/// Recent runs of the job, newest first, straight from the API (not
/// shared with `RunView`'s state, keeping this module fully decoupled).
async fn discover_runs(cli: &DatabricksCli, job_id: &str) -> Result<Vec<Value>, String> {
    let limit = RECENT_RUNS_TO_SCAN.to_string();
    let args = ["jobs", "list-runs", "--job-id", job_id, "--limit", &limit];
    let json = cli.run(&args).await.map_err(|e| format!("{e:#}"))?;
    let runs = json
        .as_array()
        .cloned()
        .or_else(|| json["runs"].as_array().cloned())
        .unwrap_or_default();
    if runs.is_empty() {
        return Err("no runs found for this job".to_string());
    }
    Ok(runs)
}

/// The cluster a `jobs get-run` response ran on: top-level first, else
/// the first task that names one. None for serverless runs.
fn extract_cluster_id(json: &Value) -> Option<String> {
    if let Some(id) = json["cluster_instance"]["cluster_id"].as_str() {
        return Some(id.to_string());
    }
    json["tasks"].as_array()?.iter().find_map(|t| {
        t["cluster_instance"]["cluster_id"]
            .as_str()
            .map(str::to_string)
    })
}

const NO_CLUSTER: &str = "this run has no attached cluster (serverless compute) — \
    Spark diagnostics need a classic cluster";

async fn discover_cluster(cli: &DatabricksCli, run_id: &str) -> Result<String, String> {
    let json = cli
        .run(&["jobs", "get-run", run_id])
        .await
        .map_err(|e| format!("{e:#}"))?;
    extract_cluster_id(&json).ok_or_else(|| NO_CLUSTER.to_string())
}

/// Confirmed against a live workspace: once a cluster is terminated,
/// this driver-proxy path fails with a clean `INVALID_STATE` error
/// rather than a network/timeout failure — the endpoint genuinely
/// requires the driver to be up, it doesn't serve cached results after
/// termination. Recognized here so the UI can say that plainly instead
/// of surfacing the raw CLI/API error text.
fn friendly_proxy_error(raw: &str) -> String {
    if raw.contains("INVALID_STATE") || raw.contains("Terminated state") {
        "this run's cluster has already terminated — the driver is gone".to_string()
    } else {
        raw.to_string()
    }
}

async fn driver_proxy_get(cli: &DatabricksCli, path: &str) -> Result<Value, String> {
    cli.run(&["api", "get", path])
        .await
        .map_err(|e| friendly_proxy_error(&format!("{e:#}")))
}

async fn list_apps(cli: &DatabricksCli, cluster_id: &str) -> Result<String, String> {
    let path = format!(
        "/driver-proxy-api/o/{DRIVER_PROXY_ORG}/{cluster_id}/{DRIVER_PROXY_PORT}/api/v1/applications"
    );
    let json = driver_proxy_get(cli, &path).await?;
    json.as_array()
        .and_then(|apps| apps.first())
        .and_then(|a| a["id"].as_str())
        .map(str::to_string)
        .ok_or_else(|| "no Spark application found on this cluster's driver".to_string())
}

async fn fetch_stages(
    cli: &DatabricksCli,
    cluster_id: &str,
    app_id: &str,
) -> Result<Vec<StageDiag>, String> {
    let path = format!(
        "/driver-proxy-api/o/{DRIVER_PROXY_ORG}/{cluster_id}/{DRIVER_PROXY_PORT}/api/v1/applications/{app_id}/stages?details=true"
    );
    let json = driver_proxy_get(cli, &path).await?;
    Ok(parse_stages(&json))
}

/// Per-stage skew and spill from Spark's own `stages?details=true`
/// response. Most recent ~10 stages, newest first.
fn parse_stages(json: &Value) -> Vec<StageDiag> {
    let stages = json.as_array().cloned().unwrap_or_default();
    let mut out: Vec<StageDiag> = stages
        .iter()
        .map(|s| {
            let mut durations: Vec<i64> = Vec::new();
            let mut memory_bytes_spilled = 0i64;
            let mut disk_bytes_spilled = 0i64;
            if let Some(tasks) = s["tasks"].as_object() {
                for t in tasks.values() {
                    if let Some(d) = t["duration"].as_i64() {
                        durations.push(d);
                    }
                    memory_bytes_spilled +=
                        t["taskMetrics"]["memoryBytesSpilled"].as_i64().unwrap_or(0);
                    disk_bytes_spilled +=
                        t["taskMetrics"]["diskBytesSpilled"].as_i64().unwrap_or(0);
                }
            }
            let (max_task_duration_ms, median_task_duration_ms, skew_ratio) =
                skew_ratio_of(durations);
            StageDiag {
                stage_id: s["stageId"].as_i64().unwrap_or(0),
                name: s["name"].as_str().unwrap_or("?").to_string(),
                num_tasks: s["numTasks"].as_i64().unwrap_or(0),
                max_task_duration_ms,
                median_task_duration_ms,
                skew_ratio,
                memory_bytes_spilled,
                disk_bytes_spilled,
            }
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.stage_id));
    out.truncate(10);
    out
}

/// Probes one run's cluster: the live driver first (fast, and the
/// freshest possible data while the cluster is up), and — only when
/// that fails specifically because the cluster has terminated — falls
/// back to reading the Spark event log Databricks delivered for it, if
/// cluster log delivery is configured.
async fn probe_run(
    cli: &DatabricksCli,
    run_id: &str,
    cluster_id: &str,
) -> Result<(String, DiagSource, Vec<StageDiag>), String> {
    match probe_driver(cli, cluster_id).await {
        Ok((app_id, stages)) => Ok((app_id, DiagSource::Live, stages)),
        Err(live_err) if live_err.contains("already terminated") => {
            match probe_event_log(cli, cluster_id).await {
                Ok((app_id, stages)) => Ok((app_id, DiagSource::EventLog, stages)),
                Err(log_err) => Err(format!("run {run_id}: {live_err}, and {log_err}")),
            }
        }
        Err(live_err) => Err(format!("run {run_id}: {live_err}")),
    }
}

/// Best-effort spill/skew diagnostics for the job's most recent run that
/// actually has a cluster to probe. Scans back through a few recent runs
/// (see `RECENT_RUNS_TO_SCAN`) since the single newest one is often
/// exactly the kind with nothing to show — skipped, disabled, a
/// condition that wasn't met — and uses the first one, newest first,
/// that either has live driver data or a readable delivered event log.
/// Every failure path returns a short, UI-safe `Err` — never panics — so
/// the caller can show it as a calm "unavailable" note rather than an
/// error.
pub async fn fetch(cli: &DatabricksCli, job_id: &str) -> Result<SparkLiveData, String> {
    let runs = discover_runs(cli, job_id).await?;
    let mut first_err: Option<String> = None;

    for run in &runs {
        let Some(run_id) = run["run_id"].as_u64().map(|n| n.to_string()) else {
            continue;
        };
        let cluster_id = match discover_cluster(cli, &run_id).await {
            Ok(id) => id,
            Err(e) => {
                first_err.get_or_insert(format!("run {run_id}: {e}"));
                continue;
            }
        };
        match probe_run(cli, &run_id, &cluster_id).await {
            Ok((app_id, source, stages)) => {
                return Ok(SparkLiveData {
                    app_id,
                    run_id,
                    source,
                    stages,
                })
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }

    Err(first_err.unwrap_or_else(|| {
        format!("none of the last {RECENT_RUNS_TO_SCAN} runs had a cluster to probe")
    }))
}

async fn probe_driver(
    cli: &DatabricksCli,
    cluster_id: &str,
) -> Result<(String, Vec<StageDiag>), String> {
    let app_id = list_apps(cli, cluster_id).await?;
    let stages = fetch_stages(cli, cluster_id, &app_id).await?;
    Ok((app_id, stages))
}

/// Destination directory Databricks delivers this cluster's logs to, if
/// cluster log delivery is configured with a DBFS destination — other
/// destination kinds (S3/ADLS/GCS) aren't readable through `fs` with
/// only Databricks CLI auth, so they're reported as unsupported rather
/// than guessed at.
fn dbfs_log_destination(json: &Value) -> Result<String, String> {
    let conf = &json["cluster_log_conf"];
    if let Some(dest) = conf["dbfs"]["destination"].as_str() {
        return Ok(dest.trim_end_matches('/').to_string());
    }
    if conf.is_object() && !conf.as_object().is_some_and(|o| o.is_empty()) {
        return Err(
            "this cluster's log delivery isn't DBFS-based — can't read it directly".to_string(),
        );
    }
    Err("this cluster has no log delivery configured — no event log to read".to_string())
}

async fn event_log_destination(cli: &DatabricksCli, cluster_id: &str) -> Result<String, String> {
    let json = cli
        .run(&["clusters", "get", cluster_id])
        .await
        .map_err(|e| format!("{e:#}"))?;
    dbfs_log_destination(&json)
}

/// One level of `fs ls`, as (basename, is_dir).
async fn list_dir(cli: &DatabricksCli, path: &str) -> Result<Vec<(String, bool)>, String> {
    let json = cli
        .run(&["fs", "ls", path])
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(json
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let name = f["name"]
                .as_str()
                .or_else(|| f["path"].as_str())?
                .trim_end_matches('/')
                .rsplit('/')
                .next()?
                .to_string();
            let is_dir = f["is_directory"]
                .as_bool()
                .or_else(|| f["is_dir"].as_bool())
                .unwrap_or(false);
            Some((name, is_dir))
        })
        .collect())
}

/// Descends `<destination>/<cluster_id>/eventlog/<app-dir>/<context-dir>/`
/// to the delivered event log file, per the directory shape Databricks
/// uses for cluster log delivery (confirmed against a live workspace):
/// one subdirectory per Spark application instance, one further
/// subdirectory per Spark context, then the file itself — usually named
/// `eventlog` (the most recent segment) alongside older timestamped
/// `.gz` segments from Spark's own log rolling, which aren't read here.
fn pick_event_log_file(dir: &str, entries: &[(String, bool)]) -> Result<String, String> {
    if entries
        .iter()
        .any(|(name, is_dir)| !is_dir && name == "eventlog")
    {
        return Ok(format!("{dir}/eventlog"));
    }
    if entries
        .iter()
        .any(|(name, is_dir)| !is_dir && name.starts_with("eventlog") && name.ends_with(".gz"))
    {
        return Err(
            "the event log has already rolled over to a compressed segment — not read here"
                .to_string(),
        );
    }
    Err(format!("no event log file found under {dir}"))
}

async fn find_event_log_file(cli: &DatabricksCli, base: &str) -> Result<String, String> {
    let mut dir = base.to_string();
    for _ in 0..2 {
        let entries = list_dir(cli, &dir).await?;
        let Some((name, _)) = entries.iter().find(|(_, is_dir)| *is_dir) else {
            return Err(format!("no subdirectory found under {dir}"));
        };
        dir = format!("{dir}/{name}");
    }
    let entries = list_dir(cli, &dir).await?;
    pick_event_log_file(&dir, &entries)
}

/// Only the tail of a huge event log matters for "most recent stages" —
/// bounds how many lines get parsed rather than risking a slow pass over
/// a job that produced millions of task events.
const MAX_EVENT_LOG_LINES: usize = 200_000;

async fn probe_event_log(
    cli: &DatabricksCli,
    cluster_id: &str,
) -> Result<(String, Vec<StageDiag>), String> {
    let dest = event_log_destination(cli, cluster_id).await?;
    let base = format!("{dest}/{cluster_id}/eventlog");
    let path = find_event_log_file(cli, &base).await?;
    let content = cli
        .run_raw(&["fs", "cat", &path])
        .await
        .map_err(|e| format!("{e:#}"))?;
    let (app_id, stages) = parse_event_log(&content);
    if stages.is_empty() {
        return Err("event log had no stage data".to_string());
    }
    Ok((app_id.unwrap_or_else(|| "unknown".to_string()), stages))
}

/// Parses a Spark event log — newline-delimited JSON, one
/// `SparkListenerEvent` per line, Apache Spark's own documented format
/// rather than a Databricks-private one — into the app id and per-stage
/// skew/spill, in the same shape `parse_stages` produces from the live
/// driver so the two sources render identically. Malformed lines (e.g.
/// a partial trailing line from a log still being written) are skipped,
/// not fatal; only the last `MAX_EVENT_LOG_LINES` are considered.
fn parse_event_log(content: &str) -> (Option<String>, Vec<StageDiag>) {
    let mut app_id: Option<String> = None;
    let mut names: HashMap<i64, String> = HashMap::new();
    let mut durations: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut mem_spill: HashMap<i64, i64> = HashMap::new();
    let mut disk_spill: HashMap<i64, i64> = HashMap::new();

    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(MAX_EVENT_LOG_LINES);

    for line in &lines[start..] {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event["Event"].as_str() {
            Some("SparkListenerApplicationStart") => {
                if let Some(id) = event["App ID"].as_str() {
                    app_id = Some(id.to_string());
                }
            }
            Some("SparkListenerStageSubmitted") | Some("SparkListenerStageCompleted") => {
                let info = &event["Stage Info"];
                if let (Some(id), Some(name)) =
                    (info["Stage ID"].as_i64(), info["Stage Name"].as_str())
                {
                    names.insert(id, name.to_string());
                }
            }
            Some("SparkListenerTaskEnd") => {
                let Some(stage_id) = event["Stage ID"].as_i64() else {
                    continue;
                };
                let info = &event["Task Info"];
                let launch = info["Launch Time"].as_i64().unwrap_or(0);
                let finish = info["Finish Time"].as_i64().unwrap_or(0);
                if finish > launch {
                    durations.entry(stage_id).or_default().push(finish - launch);
                }
                let metrics = &event["Task Metrics"];
                *mem_spill.entry(stage_id).or_insert(0) +=
                    metrics["Memory Bytes Spilled"].as_i64().unwrap_or(0);
                *disk_spill.entry(stage_id).or_insert(0) +=
                    metrics["Disk Bytes Spilled"].as_i64().unwrap_or(0);
            }
            _ => {}
        }
    }

    let mut out: Vec<StageDiag> = durations
        .into_iter()
        .map(|(stage_id, ds)| {
            let num_tasks = ds.len() as i64;
            let (max_task_duration_ms, median_task_duration_ms, skew_ratio) = skew_ratio_of(ds);
            StageDiag {
                stage_id,
                name: names
                    .get(&stage_id)
                    .cloned()
                    .unwrap_or_else(|| format!("stage {stage_id}")),
                num_tasks,
                max_task_duration_ms,
                median_task_duration_ms,
                skew_ratio,
                memory_bytes_spilled: mem_spill.get(&stage_id).copied().unwrap_or(0),
                disk_bytes_spilled: disk_spill.get(&stage_id).copied().unwrap_or(0),
            }
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.stage_id));
    out.truncate(10);
    (app_id, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dbfs_log_destination_extracts_and_trims_the_path() {
        let json = json!({"cluster_log_conf": {"dbfs": {"destination": "dbfs:/cluster-logs/"}}});
        assert_eq!(
            dbfs_log_destination(&json),
            Ok("dbfs:/cluster-logs".to_string())
        );
    }

    #[test]
    fn dbfs_log_destination_rejects_non_dbfs_destinations() {
        let json = json!({"cluster_log_conf": {"s3": {"destination": "s3://bucket/logs"}}});
        assert!(dbfs_log_destination(&json)
            .unwrap_err()
            .contains("isn't DBFS-based"));
    }

    #[test]
    fn dbfs_log_destination_reports_no_delivery_configured() {
        assert!(dbfs_log_destination(&json!({}))
            .unwrap_err()
            .contains("no log delivery configured"));
    }

    #[test]
    fn pick_event_log_file_prefers_the_plain_file() {
        let entries = vec![
            ("eventlog-2026-08-20--01-10.gz".to_string(), false),
            ("eventlog".to_string(), false),
        ];
        assert_eq!(
            pick_event_log_file("dir", &entries),
            Ok("dir/eventlog".to_string())
        );
    }

    #[test]
    fn pick_event_log_file_reports_rolled_over_when_only_gz_present() {
        let entries = vec![("eventlog-2026-08-20--01-10.gz".to_string(), false)];
        assert!(pick_event_log_file("dir", &entries)
            .unwrap_err()
            .contains("rolled over"));
    }

    #[test]
    fn pick_event_log_file_reports_nothing_found() {
        let entries = vec![("stdout".to_string(), false)];
        assert!(pick_event_log_file("dir", &entries)
            .unwrap_err()
            .contains("no event log file found"));
    }

    #[test]
    fn skew_ratio_of_flags_a_skewed_stage_and_spares_a_single_task() {
        assert_eq!(skew_ratio_of(vec![100, 100, 400]), (400, 100, 4.0));
        assert_eq!(skew_ratio_of(vec![50]), (50, 50, 0.0));
        assert_eq!(skew_ratio_of(vec![]), (0, 0, 0.0));
    }

    #[test]
    fn parse_event_log_aggregates_stages_from_task_end_events() {
        let content = [
            r#"{"Event":"SparkListenerApplicationStart","App ID":"app-123"}"#,
            r#"{"Event":"SparkListenerStageSubmitted","Stage Info":{"Stage ID":3,"Stage Name":"shuffle at File.scala:42"}}"#,
            r#"{"Event":"SparkListenerTaskEnd","Stage ID":3,"Task Info":{"Launch Time":1000,"Finish Time":1100},"Task Metrics":{"Memory Bytes Spilled":10,"Disk Bytes Spilled":0}}"#,
            r#"{"Event":"SparkListenerTaskEnd","Stage ID":3,"Task Info":{"Launch Time":1000,"Finish Time":1100},"Task Metrics":{"Memory Bytes Spilled":0,"Disk Bytes Spilled":5}}"#,
            r#"{"Event":"SparkListenerTaskEnd","Stage ID":3,"Task Info":{"Launch Time":1000,"Finish Time":1400},"Task Metrics":{"Memory Bytes Spilled":0,"Disk Bytes Spilled":0}}"#,
            "not even json",
        ]
        .join("\n");

        let (app_id, stages) = parse_event_log(&content);
        assert_eq!(app_id.as_deref(), Some("app-123"));
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].stage_id, 3);
        assert_eq!(stages[0].name, "shuffle at File.scala:42");
        assert_eq!(stages[0].num_tasks, 3);
        assert_eq!(stages[0].max_task_duration_ms, 400);
        assert_eq!(stages[0].median_task_duration_ms, 100);
        assert_eq!(stages[0].skew_ratio, 4.0);
        assert_eq!(stages[0].memory_bytes_spilled, 10);
        assert_eq!(stages[0].disk_bytes_spilled, 5);
    }

    #[test]
    fn parse_event_log_falls_back_to_a_generic_name_without_stage_info() {
        let content = r#"{"Event":"SparkListenerTaskEnd","Stage ID":7,"Task Info":{"Launch Time":0,"Finish Time":50},"Task Metrics":{}}"#;
        let (_, stages) = parse_event_log(content);
        assert_eq!(stages[0].name, "stage 7");
    }

    #[test]
    fn friendly_proxy_error_recognizes_a_terminated_cluster() {
        let raw = r#"databricks CLI error: Error: "INVALID_STATE": Cluster 0101-abc is in Terminated state"#;
        assert_eq!(
            friendly_proxy_error(raw),
            "this run's cluster has already terminated — the driver is gone"
        );
    }

    #[test]
    fn friendly_proxy_error_passes_through_anything_else() {
        assert_eq!(
            friendly_proxy_error("connection refused"),
            "connection refused"
        );
    }

    #[test]
    fn cluster_id_prefers_the_top_level_field() {
        let run = json!({
            "cluster_instance": {"cluster_id": "top"},
            "tasks": [{"cluster_instance": {"cluster_id": "task"}}],
        });
        assert_eq!(extract_cluster_id(&run).as_deref(), Some("top"));
    }

    #[test]
    fn cluster_id_falls_back_to_the_first_task() {
        let run = json!({
            "tasks": [
                {"task_key": "a"},
                {"task_key": "b", "cluster_instance": {"cluster_id": "task-b"}},
            ],
        });
        assert_eq!(extract_cluster_id(&run).as_deref(), Some("task-b"));
    }

    #[test]
    fn cluster_id_is_none_for_serverless_runs() {
        let run = json!({"tasks": [{"task_key": "a"}]});
        assert_eq!(extract_cluster_id(&run), None);
    }

    #[test]
    fn parse_stages_computes_skew_and_spill() {
        let json = json!([
            {
                "stageId": 3,
                "name": "shuffle",
                "numTasks": 3,
                "tasks": {
                    "1": {"duration": 100, "taskMetrics": {"memoryBytesSpilled": 10, "diskBytesSpilled": 0}},
                    "2": {"duration": 100, "taskMetrics": {"memoryBytesSpilled": 0, "diskBytesSpilled": 5}},
                    "3": {"duration": 400, "taskMetrics": {"memoryBytesSpilled": 0, "diskBytesSpilled": 0}},
                }
            },
            {
                "stageId": 1,
                "name": "single-task",
                "numTasks": 1,
                "tasks": {
                    "1": {"duration": 50, "taskMetrics": {}},
                }
            }
        ]);
        let stages = parse_stages(&json);
        assert_eq!(stages.len(), 2);
        // Newest (highest stage id) first.
        assert_eq!(stages[0].stage_id, 3);
        assert_eq!(stages[0].median_task_duration_ms, 100);
        assert_eq!(stages[0].max_task_duration_ms, 400);
        assert_eq!(stages[0].skew_ratio, 4.0);
        assert_eq!(stages[0].memory_bytes_spilled, 10);
        assert_eq!(stages[0].disk_bytes_spilled, 5);
        // A single-task stage doesn't produce a misleading skew ratio.
        assert_eq!(stages[1].skew_ratio, 0.0);
    }

    #[test]
    fn parse_stages_tolerates_missing_task_data() {
        let json = json!([{"stageId": 1, "name": "x", "numTasks": 0}]);
        let stages = parse_stages(&json);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].skew_ratio, 0.0);
    }
}
