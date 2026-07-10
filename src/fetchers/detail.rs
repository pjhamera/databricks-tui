use crate::cli::DatabricksCli;
use crate::shape::{fmt_duration_ms, relative_time, DetailData, Status};
use serde_json::Value;

/// Fetches the full detail view for one resource: key facts, recent
/// activity, and the raw JSON. Never fails — errors land in `raw`.
pub async fn fetch(cli: &DatabricksCli, group: &str, id: &str) -> DetailData {
    // The activity call is independent of `get` — run them concurrently.
    let verb = if group == "volumes" { "read" } else { "get" };
    let get_args = [group, verb, id];
    let get = cli.run(&get_args);
    let (main, mut activity) = match group {
        "clusters" => tokio::join!(get, cluster_events(cli, id)),
        "jobs" => tokio::join!(get, job_runs(cli, id)),
        "warehouses" => tokio::join!(get, warehouse_queries(cli, id)),
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
        "lakeview" => {
            activity = dashboard_contents(&json);
            dashboard_summary(&json)
        }
        "tables" => {
            activity = table_columns(&json);
            table_summary(&json)
        }
        "volumes" => volume_summary(&json),
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

fn dashboard_summary(j: &Value) -> Vec<(String, String)> {
    let mut s = Vec::new();
    push(&mut s, "State", str_of(&j["lifecycle_state"]));
    push(&mut s, "Path", str_of(&j["path"]));
    push(&mut s, "Warehouse", str_of(&j["warehouse_id"]));
    push(&mut s, "Updated", str_of(&j["update_time"]));
    if let Some(def) = parse_serialized(j) {
        push(
            &mut s,
            "Pages",
            def["pages"].as_array().map(|p| p.len().to_string()),
        );
        push(
            &mut s,
            "Datasets",
            def["datasets"].as_array().map(|d| d.len().to_string()),
        );
    }
    s
}

/// The dashboard definition is embedded as a JSON string inside the response.
fn parse_serialized(j: &Value) -> Option<Value> {
    j["serialized_dashboard"]
        .as_str()
        .and_then(|s| serde_json::from_str(s).ok())
}

/// Pages and widget titles, best effort — Lakeview layouts nest deeply.
fn dashboard_contents(j: &Value) -> Vec<(Status, String)> {
    let Some(def) = parse_serialized(j) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for page in def["pages"].as_array().into_iter().flatten() {
        let page_name = page["displayName"]
            .as_str()
            .or_else(|| page["name"].as_str())
            .unwrap_or("page");
        out.push((Status::Running, format!("▤ {page_name}")));
        for item in page["layout"].as_array().into_iter().flatten() {
            let widget = &item["widget"];
            let title = widget["spec"]["frame"]["title"]
                .as_str()
                .filter(|t| !t.is_empty())
                .or_else(|| widget["name"].as_str())
                .unwrap_or("widget");
            let kind = widget["spec"]["widgetType"].as_str().unwrap_or("");
            let line = if kind.is_empty() {
                format!("  · {title}")
            } else {
                format!("  · {title} ({kind})")
            };
            out.push((Status::Unknown(String::new()), line));
        }
    }
    out.truncate(30);
    out
}

fn table_summary(j: &Value) -> Vec<(String, String)> {
    let mut s = Vec::new();
    push(&mut s, "Type", str_of(&j["table_type"]));
    push(&mut s, "Format", str_of(&j["data_source_format"]));
    push(&mut s, "Owner", str_of(&j["owner"]));
    push(&mut s, "Location", str_of(&j["storage_location"]));
    push(&mut s, "Comment", str_of(&j["comment"]));
    push(
        &mut s,
        "Updated",
        j["updated_at"].as_u64().map(relative_time),
    );
    s
}

/// Column list rendered as the activity section of a table detail.
fn table_columns(j: &Value) -> Vec<(Status, String)> {
    j["columns"]
        .as_array()
        .map(|cols| {
            cols.iter()
                .map(|c| {
                    let name = c["name"].as_str().unwrap_or("?");
                    let ty = c["type_text"]
                        .as_str()
                        .or_else(|| c["type_name"].as_str())
                        .unwrap_or("");
                    (Status::Unknown(String::new()), format!("{name}  ·  {ty}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn volume_summary(j: &Value) -> Vec<(String, String)> {
    let mut s = Vec::new();
    push(&mut s, "Type", str_of(&j["volume_type"]));
    push(&mut s, "Owner", str_of(&j["owner"]));
    push(&mut s, "Location", str_of(&j["storage_location"]));
    push(&mut s, "Comment", str_of(&j["comment"]));
    push(
        &mut s,
        "Updated",
        j["updated_at"].as_u64().map(relative_time),
    );
    s
}

/// Recent queries on a warehouse, via the query history REST API —
/// no warehouse wake-up needed.
async fn warehouse_queries(cli: &DatabricksCli, id: &str) -> Vec<(Status, String)> {
    let path = format!("/api/2.0/sql/history/queries?filter_by.warehouse_ids={id}&max_results=12");
    let Ok(json) = cli.run(&["api", "get", &path]).await else {
        return Vec::new();
    };
    json["res"]
        .as_array()
        .map(|queries| {
            queries
                .iter()
                .map(|q| {
                    let status: Status = q["status"].as_str().unwrap_or("").parse().unwrap();
                    let user = q["user_display_name"]
                        .as_str()
                        .or_else(|| q["user_name"].as_str())
                        .unwrap_or("?");
                    let when = q["query_start_time_ms"]
                        .as_u64()
                        .map(relative_time)
                        .unwrap_or_default();
                    let dur = q["duration"]
                        .as_u64()
                        .map(fmt_duration_ms)
                        .unwrap_or_default();
                    let text: String = q["query_text"]
                        .as_str()
                        .unwrap_or("")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(60)
                        .collect();
                    let parts: Vec<String> = [user.to_string(), dur, when, text]
                        .into_iter()
                        .filter(|p| !p.is_empty())
                        .collect();
                    (status, parts.join(" · "))
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
