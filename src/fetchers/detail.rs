use crate::cli::DatabricksCli;
use crate::shape::{fmt_duration_ms, relative_time, DetailData, Status};
use serde_json::Value;

/// Fetches the full detail view for one resource: key facts, recent
/// activity, and the raw JSON. Never fails — errors land in `raw`.
pub async fn fetch(cli: &DatabricksCli, group: &str, id: &str) -> DetailData {
    // The activity call is independent of `get` — run them concurrently.
    let get_args = [group, "get", id];
    let get = cli.run(&get_args);
    let (main, mut activity) = match group {
        "clusters" => tokio::join!(get, cluster_events(cli, id)),
        "jobs" => tokio::join!(get, job_runs(cli, id)),
        _ => (get.await, Vec::new()),
    };
    let json = match main {
        Ok(v) => v,
        Err(e) => {
            return DetailData {
                summary: Vec::new(),
                activity: Vec::new(),
                raw: format!("{e:#}"),
            }
        }
    };
    let raw = serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string());

    let summary = match group {
        "clusters" => cluster_summary(&json),
        "jobs" => job_summary(&json),
        "pipelines" => {
            activity = pipeline_updates(&json);
            pipeline_summary(&json)
        }
        _ => warehouse_summary(&json),
    };

    DetailData {
        summary,
        activity,
        raw,
    }
}

fn push(out: &mut Vec<(String, String)>, key: &str, value: Option<String>) {
    if let Some(v) = value {
        if !v.is_empty() {
            out.push((key.to_string(), v));
        }
    }
}

fn str_of(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn cluster_summary(j: &Value) -> Vec<(String, String)> {
    let mut s = Vec::new();
    push(&mut s, "State", str_of(&j["state"]));
    push(&mut s, "Spark", str_of(&j["spark_version"]));
    push(&mut s, "Node type", str_of(&j["node_type_id"]));
    let workers = j["num_workers"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| {
            let (min, max) = (
                j["autoscale"]["min_workers"].as_u64()?,
                j["autoscale"]["max_workers"].as_u64()?,
            );
            Some(format!("{}–{} (autoscale)", min, max))
        });
    push(&mut s, "Workers", workers);
    push(&mut s, "Creator", str_of(&j["creator_user_name"]));
    push(
        &mut s,
        "Started",
        j["start_time"].as_u64().map(relative_time),
    );
    push(
        &mut s,
        "Auto-terminate",
        j["autotermination_minutes"]
            .as_u64()
            .map(|m| format!("{m} min")),
    );
    s
}

async fn cluster_events(cli: &DatabricksCli, id: &str) -> Vec<(Status, String)> {
    let Ok(json) = cli.run(&["clusters", "events", id]).await else {
        return Vec::new();
    };
    json["events"]
        .as_array()
        .map(|events| {
            events
                .iter()
                .take(8)
                .map(|e| {
                    let kind = e["type"].as_str().unwrap_or("EVENT");
                    let when = e["timestamp"]
                        .as_u64()
                        .map(relative_time)
                        .unwrap_or_default();
                    (kind.parse().unwrap(), format!("{kind} · {when}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn job_summary(j: &Value) -> Vec<(String, String)> {
    let mut s = Vec::new();
    let settings = &j["settings"];
    push(&mut s, "Creator", str_of(&j["creator_user_name"]));
    let schedule = settings["schedule"]["quartz_cron_expression"]
        .as_str()
        .map(|cron| {
            let tz = settings["schedule"]["timezone_id"]
                .as_str()
                .unwrap_or("UTC");
            let paused = settings["schedule"]["pause_status"].as_str().unwrap_or("");
            let suffix = if paused == "PAUSED" { " (paused)" } else { "" };
            format!("{cron} {tz}{suffix}")
        });
    push(&mut s, "Schedule", schedule);
    push(
        &mut s,
        "Tasks",
        settings["tasks"].as_array().map(|t| t.len().to_string()),
    );
    push(
        &mut s,
        "Max concurrent",
        settings["max_concurrent_runs"]
            .as_u64()
            .map(|n| n.to_string()),
    );
    s
}

async fn job_runs(cli: &DatabricksCli, id: &str) -> Vec<(Status, String)> {
    let Ok(json) = cli
        .run(&["jobs", "list-runs", "--job-id", id, "--limit", "10"])
        .await
    else {
        return Vec::new();
    };
    json.as_array()
        .map(|runs| {
            runs.iter()
                .take(10)
                .map(|r| {
                    let status: Status = r["state"]["result_state"]
                        .as_str()
                        .or_else(|| r["state"]["life_cycle_state"].as_str())
                        .unwrap_or("")
                        .parse()
                        .unwrap();
                    let when = r["start_time"]
                        .as_u64()
                        .map(relative_time)
                        .unwrap_or_default();
                    let dur = r["run_duration"]
                        .as_u64()
                        .or_else(|| r["execution_duration"].as_u64())
                        .map(fmt_duration_ms)
                        .unwrap_or_default();
                    let label = status.label().to_string();
                    let text = [label, when, dur]
                        .into_iter()
                        .filter(|p| !p.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    (status, text)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pipeline_summary(j: &Value) -> Vec<(String, String)> {
    let mut s = Vec::new();
    push(&mut s, "State", str_of(&j["state"]));
    push(
        &mut s,
        "Mode",
        j["spec"]["continuous"]
            .as_bool()
            .map(|c| if c { "continuous" } else { "triggered" }.to_string()),
    );
    push(&mut s, "Channel", str_of(&j["spec"]["channel"]));
    push(&mut s, "Edition", str_of(&j["spec"]["edition"]));
    push(&mut s, "Catalog", str_of(&j["spec"]["catalog"]));
    push(&mut s, "Target", str_of(&j["spec"]["target"]));
    push(&mut s, "Creator", str_of(&j["creator_user_name"]));
    s
}

/// `pipelines get` already includes recent updates — no extra call needed.
fn pipeline_updates(j: &Value) -> Vec<(Status, String)> {
    j["latest_updates"]
        .as_array()
        .map(|updates| {
            updates
                .iter()
                .take(8)
                .map(|u| {
                    let state = u["state"].as_str().unwrap_or("UNKNOWN");
                    let when = u["creation_time"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_default();
                    (state.parse().unwrap(), format!("{state} · {when}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn warehouse_summary(j: &Value) -> Vec<(String, String)> {
    let mut s = Vec::new();
    push(&mut s, "State", str_of(&j["state"]));
    push(&mut s, "Size", str_of(&j["cluster_size"]));
    push(
        &mut s,
        "Clusters",
        j["num_clusters"].as_u64().map(|n| n.to_string()),
    );
    push(
        &mut s,
        "Scaling",
        match (
            j["min_num_clusters"].as_u64(),
            j["max_num_clusters"].as_u64(),
        ) {
            (Some(min), Some(max)) => Some(format!("{min}–{max}")),
            _ => None,
        },
    );
    push(
        &mut s,
        "Auto-stop",
        j["auto_stop_mins"].as_u64().map(|m| format!("{m} min")),
    );
    push(
        &mut s,
        "Serverless",
        j["enable_serverless_compute"]
            .as_bool()
            .map(|b| b.to_string()),
    );
    push(&mut s, "Creator", str_of(&j["creator_name"]));
    s
}
