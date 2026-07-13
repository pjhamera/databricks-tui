use crate::cli::DatabricksCli;
use crate::shape::{relative_time, DetailData, Status};
use serde_json::Value;

fn update_status(u: &Value) -> Status {
    u["state"].as_str().unwrap_or("").parse().unwrap()
}

/// Age of an update; the API returns epoch millis, but tolerate ISO
/// strings by showing their date part.
fn age_of(u: &Value) -> String {
    match &u["creation_time"] {
        Value::Number(n) => n.as_u64().map(relative_time).unwrap_or_default(),
        Value::String(s) => s.chars().take(16).collect(),
        _ => String::new(),
    }
}

/// Recent updates of one pipeline, newest first: (update_id, status, age).
pub async fn list(
    cli: &DatabricksCli,
    pipeline_id: &str,
) -> Result<Vec<(String, Status, String)>, String> {
    let args = [
        "pipelines",
        "list-updates",
        pipeline_id,
        "--max-results",
        "20",
    ];
    let json = cli.run(&args).await.map_err(|e| format!("{e:#}"))?;
    Ok(json["updates"]
        .as_array()
        .map(|updates| {
            updates
                .iter()
                .filter_map(|u| {
                    let id = u["update_id"].as_str()?;
                    Some((id.to_string(), update_status(u), age_of(u)))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Full detail of one update: state, cause, timing, and its event-log
/// entries (errors highlighted). The bool is true while the update is
/// still executing.
pub async fn fetch(cli: &DatabricksCli, pipeline_id: &str, update_id: &str) -> (DetailData, bool) {
    let get_args = ["pipelines", "get-update", pipeline_id, update_id];
    let events_args = [
        "pipelines",
        "list-pipeline-events",
        pipeline_id,
        "--max-results",
        "100",
    ];
    let (update, events) = tokio::join!(cli.run(&get_args), cli.run(&events_args));

    let json = match update {
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
    let u = if json["update"].is_object() {
        &json["update"]
    } else {
        &json
    };
    let raw = serde_json::to_string_pretty(u).unwrap_or_else(|_| u.to_string());

    let status = update_status(u);
    let live = matches!(status, Status::Running | Status::Pending);
    let mut summary = vec![(
        "State".to_string(),
        u["state"].as_str().unwrap_or("?").to_string(),
    )];
    if let Some(cause) = u["cause"].as_str() {
        summary.push(("Cause".to_string(), cause.to_string()));
    }
    let created = age_of(u);
    if !created.is_empty() {
        summary.push(("Created".to_string(), created));
    }
    if let Some(fr) = u["full_refresh"].as_bool() {
        summary.push((
            "Full refresh".to_string(),
            if fr { "yes" } else { "no" }.to_string(),
        ));
    }

    // Event-log entries for this update, newest first; errors carry the
    // reason a run of the pipeline actually broke.
    let mut activity: Vec<(Status, String)> = Vec::new();
    if let Ok(ev) = events {
        if let Some(arr) = ev["events"].as_array() {
            for e in arr {
                if e["origin"]["update_id"].as_str() != Some(update_id) {
                    continue;
                }
                let level = e["level"].as_str().unwrap_or("INFO");
                let mut msg = e["message"].as_str().unwrap_or("").replace('\n', " ");
                if msg.chars().count() > 200 {
                    msg = msg.chars().take(200).collect::<String>() + "…";
                }
                let status = match level {
                    "ERROR" => Status::Failed,
                    "WARN" => Status::Pending,
                    _ => Status::Unknown(String::new()),
                };
                activity.push((status, msg));
                if activity.len() >= 12 {
                    break;
                }
            }
        }
    }
    if activity.is_empty() {
        activity.push((
            status,
            "no event-log entries recorded for this update".to_string(),
        ));
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
