use crate::cli::DatabricksCli;
use crate::shape::{relative_time, ListItem, Shape, Status};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Max concurrent per-job run queries — keeps the number of `databricks`
/// subprocesses in flight bounded on large workspaces.
const CONCURRENCY: usize = 8;
/// Recent runs to pull per job: latest result, a short history strip,
/// and enough success durations to judge whether a live run is slow.
const RUNS_PER_JOB: &str = "8";

/// Status of a single run, preferring the final result over the lifecycle state.
pub(crate) fn run_status(r: &Value) -> Status {
    r["state"]["result_state"]
        .as_str()
        .or_else(|| r["state"]["life_cycle_state"].as_str())
        .or_else(|| r["status"]["state"].as_str())
        .unwrap_or("")
        .parse()
        .unwrap()
}

/// `jobs list-runs --output json` unwraps to a bare array; tolerate the
/// wrapped `{"runs":[...]}` shape too.
fn runs_of(json: &Value) -> Vec<Value> {
    json.as_array()
        .cloned()
        .or_else(|| json["runs"].as_array().cloned())
        .unwrap_or_default()
}

pub async fn fetch(cli: &DatabricksCli) -> Result<Shape> {
    let jobs = cli.run(&["jobs", "list"]).await?;
    let Some(jobs) = jobs.as_array() else {
        return Ok(Shape::List(Vec::new()));
    };

    // A single global list-runs only returns the newest ~25 runs across
    // the whole workspace, so jobs whose latest run isn't in that window
    // wrongly show "NO RUNS". Fetch each job's own recent runs instead,
    // with bounded concurrency.
    let sem = Arc::new(Semaphore::new(CONCURRENCY));
    let mut tasks = Vec::with_capacity(jobs.len());
    for j in jobs {
        let job_id = j["job_id"].as_u64();
        let name = j["settings"]["name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let settings = j["settings"].clone();
        let cli = cli.clone();
        let sem = Arc::clone(&sem);
        tasks.push(tokio::spawn(async move {
            let mut runs: Vec<Value> = Vec::new();
            if let Some(id) = job_id {
                let _permit = sem.acquire().await;
                let id = id.to_string();
                let args = [
                    "jobs",
                    "list-runs",
                    "--job-id",
                    &id,
                    "--limit",
                    RUNS_PER_JOB,
                ];
                if let Ok(json) = cli.run(&args).await {
                    runs = runs_of(&json);
                }
            }
            build_item(name, job_id, &runs, &settings)
        }));
    }

    let mut items = Vec::with_capacity(tasks.len());
    for t in tasks {
        if let Ok(item) = t.await {
            items.push(item);
        }
    }
    Ok(Shape::List(items))
}

/// Pauses or resumes whatever makes the job fire on its own — its cron
/// schedule, continuous mode, or trigger. Returns the flash message.
/// The update API replaces a `new_settings` field wholesale, so the
/// whole block is sent back with only `pause_status` flipped.
pub async fn toggle_pause(cli: &DatabricksCli, job_id: &str, name: &str) -> Result<String, String> {
    let job = cli
        .run(&["jobs", "get", job_id])
        .await
        .map_err(|e| format!("✗ {e:#}"))?;
    let settings = &job["settings"];
    let field = ["schedule", "continuous", "trigger"]
        .into_iter()
        .find(|f| settings[*f].is_object())
        .ok_or_else(|| format!("✗ “{name}” only runs on demand — nothing to pause"))?;
    let mut block = settings[field].clone();
    let paused = block["pause_status"].as_str() == Some("PAUSED");
    block["pause_status"] = Value::String(if paused { "UNPAUSED" } else { "PAUSED" }.to_string());
    let id: u64 = job_id
        .parse()
        .map_err(|_| format!("✗ bad job id: {job_id}"))?;
    let payload = serde_json::json!({"job_id": id, "new_settings": {field: block}}).to_string();
    cli.run_action(&["jobs", "update", "--json", &payload])
        .await
        .map_err(|e| format!("✗ {e:#}"))?;
    Ok(if paused {
        format!("▶ resumed “{name}” — {field} active again")
    } else {
        format!("⏸ paused “{name}” — won't fire until resumed (S)")
    })
}

/// How a manual run's parameter overrides are passed to `run-now`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ParamKind {
    /// Job-level parameters (`settings.parameters`) → `job_parameters`.
    Job,
    /// Task notebook `base_parameters` → `notebook_params`.
    Notebook,
}

impl ParamKind {
    pub fn payload_key(self) -> &'static str {
        match self {
            ParamKind::Job => "job_parameters",
            ParamKind::Notebook => "notebook_params",
        }
    }
}

/// The job's current parameter defaults, for prefilling the run-with-
/// parameters prompt: job-level parameters when defined, otherwise the
/// notebook base_parameters merged across tasks.
pub async fn params(
    cli: &DatabricksCli,
    job_id: &str,
) -> Result<(Vec<(String, String)>, ParamKind), String> {
    let job = cli
        .run(&["jobs", "get", job_id])
        .await
        .map_err(|e| format!("✗ {e:#}"))?;
    Ok(params_of(&job["settings"]))
}

fn params_of(settings: &Value) -> (Vec<(String, String)>, ParamKind) {
    if let Some(defs) = settings["parameters"].as_array() {
        let pairs = defs
            .iter()
            .filter_map(|d| {
                let name = d["name"].as_str()?;
                Some((name.to_string(), d["default"].as_str().unwrap_or("").into()))
            })
            .collect();
        return (pairs, ParamKind::Job);
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for t in settings["tasks"].as_array().into_iter().flatten() {
        if let Some(base) = t["notebook_task"]["base_parameters"].as_object() {
            for (k, v) in base {
                if !pairs.iter().any(|(name, _)| name == k) {
                    pairs.push((k.clone(), v.as_str().unwrap_or("").to_string()));
                }
            }
        }
    }
    (pairs, ParamKind::Notebook)
}

/// Duration of a finished run in ms, whichever field the API filled in.
fn duration_of(r: &Value) -> Option<u64> {
    r["execution_duration"]
        .as_u64()
        .or_else(|| r["run_duration"].as_u64())
        .filter(|d| *d > 0)
}

/// Some((ratio, elapsed_ms, median_ms)) when the newest run is still
/// executing and has already taken at least 1.5× the median of three or
/// more earlier successful runs. Failures announce themselves; a hung
/// run just sits there looking green — this is what spots it.
fn running_long(runs: &[Value], now_ms: u64) -> Option<(f64, u64, u64)> {
    let newest = runs.first()?;
    if !matches!(run_status(newest), Status::Running) {
        return None;
    }
    let elapsed = now_ms.saturating_sub(newest["start_time"].as_u64()?);
    let mut successes: Vec<u64> = runs[1..]
        .iter()
        .filter(|r| matches!(run_status(r), Status::Success))
        .filter_map(duration_of)
        .collect();
    if successes.len() < 3 {
        return None;
    }
    successes.sort_unstable();
    let median = successes[successes.len() / 2];
    (median > 0 && elapsed * 2 >= median * 3)
        .then(|| (elapsed as f64 / median as f64, elapsed, median))
}

fn build_item(name: String, job_id: Option<u64>, runs: &[Value], settings: &Value) -> ListItem {
    let status = runs
        .first()
        .map(run_status)
        .unwrap_or(Status::Unknown("NO RUNS".to_string()));
    let history: Vec<Status> = runs.iter().take(5).rev().map(run_status).collect();
    let last_start = runs.first().and_then(|r| r["start_time"].as_u64());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // "1h ago · ⏱ in 27m" — when the job last ran and when it runs next.
    let next =
        crate::schedule::next_run(settings, last_start, now).map(|n| format!("⏱ {}", n.label));
    let mut detail = match (last_start.map(relative_time), next) {
        (Some(ago), Some(next)) => Some(format!("{ago} · {next}")),
        (ago, next) => ago.or(next),
    };
    let slow = running_long(runs, now);
    if let Some((ratio, _, _)) = slow {
        // Prefixed so pane-width truncation keeps the warning visible.
        let tag = format!("⚠ {ratio:.1}× usual");
        detail = Some(match detail {
            Some(d) => format!("{tag} · {d}"),
            None => tag,
        });
    }
    ListItem {
        name,
        status,
        detail,
        id: job_id.map(|id| id.to_string()),
        history,
        alert: slow.map(|(ratio, elapsed, median)| {
            format!(
                "running {ratio:.1}× longer than usual — {} vs ~{} median",
                crate::shape::fmt_duration_ms(elapsed),
                crate::shape::fmt_duration_ms(median)
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{params_of, running_long, ParamKind};
    use serde_json::json;

    fn run(state: &str, start: u64, dur: u64) -> serde_json::Value {
        if state == "RUNNING" {
            json!({"start_time": start, "state": {"life_cycle_state": "RUNNING"}})
        } else {
            json!({"start_time": start, "execution_duration": dur,
                   "state": {"result_state": state}})
        }
    }

    #[test]
    fn running_long_flags_only_clear_overruns() {
        let history = vec![
            run("SUCCESS", 3000, 100),
            run("SUCCESS", 2000, 100),
            run("SUCCESS", 1000, 100),
        ];
        let mut runs = vec![run("RUNNING", 10_000, 0)];
        runs.extend(history.clone());
        // 160 elapsed vs median 100 → 1.6× flagged.
        let (ratio, elapsed, median) = running_long(&runs, 10_160).unwrap();
        assert_eq!((elapsed, median), (160, 100));
        assert!((ratio - 1.6).abs() < 0.01);
        // 1.4× is within normal variance.
        assert!(running_long(&runs, 10_140).is_none());
        // Fewer than three successful samples: no baseline.
        assert!(running_long(&runs[..3], 10_600).is_none());
        // Newest run finished: nothing live to warn about.
        assert!(running_long(&history, 10_600).is_none());
    }

    #[test]
    fn running_long_ignores_failed_durations() {
        // Failed runs stop early; counting them would shrink the median.
        let runs = vec![
            run("RUNNING", 10_000, 0),
            run("FAILED", 4000, 5),
            run("SUCCESS", 3000, 100),
            run("SUCCESS", 2000, 100),
            run("SUCCESS", 1000, 100),
        ];
        assert!(running_long(&runs, 10_140).is_none());
        assert!(running_long(&runs, 10_160).is_some());
    }

    #[test]
    fn params_prefer_job_level_over_notebook() {
        let settings = json!({
            "parameters": [{"name": "date", "default": "2026-07-18"}, {"name": "mode"}],
            "tasks": [{"notebook_task": {"base_parameters": {"x": "1"}}}]
        });
        let (pairs, kind) = params_of(&settings);
        assert_eq!(kind, ParamKind::Job);
        assert_eq!(
            pairs,
            vec![
                ("date".to_string(), "2026-07-18".to_string()),
                ("mode".to_string(), String::new())
            ]
        );

        let settings = json!({
            "tasks": [
                {"notebook_task": {"base_parameters": {"env": "prod"}}},
                {"notebook_task": {"base_parameters": {"env": "dev", "day": "mon"}}},
                {"spark_python_task": {"python_file": "x.py"}}
            ]
        });
        let (pairs, kind) = params_of(&settings);
        assert_eq!(kind, ParamKind::Notebook);
        // First occurrence wins on duplicate keys across tasks.
        assert_eq!(
            pairs,
            vec![
                ("env".to_string(), "prod".to_string()),
                ("day".to_string(), "mon".to_string())
            ]
        );
    }
}
