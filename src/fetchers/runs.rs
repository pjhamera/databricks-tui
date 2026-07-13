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
