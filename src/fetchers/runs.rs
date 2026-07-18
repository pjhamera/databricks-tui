use crate::cli::DatabricksCli;
use crate::fetchers::jobs::run_status;
use crate::shape::{fmt_duration_ms, relative_time, DetailData, Status};

/// Recent runs of one job, newest first: (run_id, status, age).
pub async fn list(
    cli: &DatabricksCli,
    job_id: &str,
) -> Result<Vec<(String, Status, String)>, String> {
    let args = ["jobs", "list-runs", "--job-id", job_id, "--limit", "20"];
    let json = cli.run(&args).await.map_err(|e| format!("{e:#}"))?;
    Ok(json
        .as_array()
        .map(|runs| {
            runs.iter()
                .filter_map(|r| {
                    let id = r["run_id"].as_u64()?;
                    let age = r["start_time"].as_u64().map(relative_time)?;
                    Some((id.to_string(), run_status(r), age))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// One task's execution window, for the timeline view.
pub struct TimelineTask {
    pub key: String,
    /// Epoch ms; 0 when the task hasn't started.
    pub start: u64,
    /// None while the task is still executing.
    pub end: Option<u64>,
    pub status: Status,
}

/// Per-task execution windows parsed from a stored get-run response,
/// sorted by start time with not-yet-started tasks last.
pub fn timeline(raw: &str) -> Vec<TimelineTask> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let mut tasks: Vec<TimelineTask> = json["tasks"]
        .as_array()
        .map(|ts| {
            ts.iter()
                .map(|t| {
                    let start = t["start_time"].as_u64().unwrap_or(0);
                    let end = t["end_time"].as_u64().filter(|e| *e > 0 && start > 0);
                    TimelineTask {
                        key: t["task_key"].as_str().unwrap_or("?").to_string(),
                        start,
                        end,
                        status: run_status(t),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    tasks.sort_by_key(|t| if t.start == 0 { u64::MAX } else { t.start });
    tasks
}

/// Complete output of a run, task by task: the full error, stack trace
/// and log tail from `jobs get-run-output`. One CLI call per task, so
/// this is fetched on demand when the user opens the output view.
pub async fn full_output(cli: &DatabricksCli, run_id: &str) -> String {
    let json = match cli.run(&["jobs", "get-run", run_id]).await {
        Ok(v) => v,
        Err(e) => return format!("✗ {e:#}"),
    };
    // Multi-task runs carry per-task run ids; a legacy single-task run
    // is its own task.
    let tasks: Vec<(String, String, Status)> = match json["tasks"].as_array() {
        Some(ts) if !ts.is_empty() => ts
            .iter()
            .filter_map(|t| {
                Some((
                    t["task_key"].as_str().unwrap_or("task").to_string(),
                    t["run_id"].as_u64()?.to_string(),
                    run_status(t),
                ))
            })
            .collect(),
        _ => vec![("run".to_string(), run_id.to_string(), run_status(&json))],
    };

    let mut out = String::new();
    for (key, id, status) in &tasks {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("── {key} · {} ──\n", status.label()));
        match cli.run(&["jobs", "get-run-output", id]).await {
            Ok(o) => {
                let mut wrote = false;
                if let Some(err) = o["error"].as_str().filter(|s| !s.is_empty()) {
                    out.push_str(err.trim_end());
                    out.push('\n');
                    wrote = true;
                }
                if let Some(trace) = o["error_trace"].as_str().filter(|s| !s.is_empty()) {
                    out.push('\n');
                    out.push_str(trace.trim_end());
                    out.push('\n');
                    wrote = true;
                }
                if let Some(result) = o["notebook_output"]["result"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                {
                    out.push_str("notebook result: ");
                    out.push_str(result.trim_end());
                    out.push('\n');
                    wrote = true;
                }
                if let Some(logs) = o["logs"].as_str().filter(|s| !s.is_empty()) {
                    // Keep the tail — that's where the failure is.
                    let tail: Vec<&str> = logs.lines().rev().take(200).collect();
                    out.push_str("logs (tail):\n");
                    for line in tail.iter().rev() {
                        out.push_str(line);
                        out.push('\n');
                    }
                    wrote = true;
                }
                if !wrote {
                    out.push_str("(no output recorded for this task)\n");
                }
            }
            // Running tasks have no output yet; say why instead of nothing.
            Err(e) => {
                let msg = format!("{e:#}");
                let first = msg.lines().next().unwrap_or("no output available");
                out.push_str(&format!("({first})\n"));
            }
        }
    }
    out
}

/// Full detail of one run: state, timing, per-task results, and the
/// actual error output for failed tasks. The bool is true while the
/// run is still executing.
pub async fn fetch(cli: &DatabricksCli, run_id: &str) -> (DetailData, bool) {
    let args = ["jobs", "get-run", run_id];
    let json = match cli.run(&args).await {
        Ok(v) => v,
        Err(e) => {
            return (
                DetailData {
                    summary: Vec::new(),
                    activity: Vec::new(),
                    raw: format!("✗ {e:#}"),
                },
                false,
            )
        }
    };
    let raw = serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string());

    let life = json["state"]["life_cycle_state"].as_str().unwrap_or("");
    let result = json["state"]["result_state"].as_str().unwrap_or("");
    let state_label = if result.is_empty() { life } else { result };
    let status: Status = state_label.parse().unwrap();
    let live = matches!(status, Status::Running | Status::Pending);

    let mut summary = vec![("State".to_string(), state_label.to_string())];
    if let Some(t) = json["start_time"].as_u64() {
        summary.push(("Started".to_string(), relative_time(t)));
    }
    if let Some(d) = json["run_duration"]
        .as_u64()
        .or_else(|| json["execution_duration"].as_u64())
        .filter(|d| *d > 0)
    {
        summary.push(("Duration".to_string(), fmt_duration_ms(d)));
    }
    if let Some(trigger) = json["trigger"].as_str() {
        summary.push(("Trigger".to_string(), trigger.to_string()));
    }
    if let Some(msg) = json["state"]["state_message"]
        .as_str()
        .filter(|m| !m.is_empty())
    {
        summary.push(("Message".to_string(), msg.to_string()));
    }

    // One line per task; failed tasks get their error output inline so
    // the reason is readable without leaving the terminal.
    let mut activity: Vec<(Status, String)> = Vec::new();
    let tasks = json["tasks"].as_array().cloned().unwrap_or_default();
    for t in &tasks {
        let key = t["task_key"].as_str().unwrap_or("?");
        let t_status = run_status(t);
        let dur = t["execution_duration"]
            .as_u64()
            .or_else(|| t["run_duration"].as_u64())
            .filter(|d| *d > 0)
            .map(|d| format!("  ·  {}", fmt_duration_ms(d)))
            .unwrap_or_default();
        let line = format!("{key}  ·  {}{dur}", t_status.label());
        let failed = matches!(t_status, Status::Failed);
        activity.push((t_status, line));
        if failed {
            if let Some(task_run_id) = t["run_id"].as_u64() {
                let id = task_run_id.to_string();
                let out_args = ["jobs", "get-run-output", &id];
                if let Ok(out) = cli.run(&out_args).await {
                    if let Some(err) = out["error"].as_str() {
                        let mut msg = err.replace('\n', " ");
                        if msg.chars().count() > 200 {
                            msg = msg.chars().take(200).collect::<String>() + "…";
                        }
                        activity.push((Status::Failed, format!("  ↳ {msg}")));
                    }
                }
            }
        }
    }
    if activity.is_empty() {
        activity.push((status, "single-task run — see raw for details".to_string()));
    }

    (
        DetailData {
            summary,
            activity,
            raw,
        },
        live,
    )
}

#[cfg(test)]
mod tests {
    use super::timeline;
    use crate::shape::Status;

    #[test]
    fn timeline_sorts_by_start_with_unstarted_last() {
        let raw = r#"{"tasks":[
            {"task_key":"b","start_time":2000,"end_time":5000,"state":{"result_state":"SUCCESS"}},
            {"task_key":"c","start_time":0,"state":{"life_cycle_state":"BLOCKED"}},
            {"task_key":"a","start_time":1000,"end_time":0,"state":{"life_cycle_state":"RUNNING"}}
        ]}"#;
        let ts = timeline(raw);
        let keys: Vec<&str> = ts.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["a", "b", "c"]);
        // end_time 0 means still executing.
        assert_eq!(ts[0].end, None);
        assert!(matches!(ts[0].status, Status::Running));
        assert_eq!(ts[1].end, Some(5000));
        assert_eq!(ts[2].start, 0);
    }

    #[test]
    fn timeline_tolerates_non_json_and_taskless_runs() {
        assert!(timeline("✗ boom").is_empty());
        assert!(timeline("{}").is_empty());
    }
}
