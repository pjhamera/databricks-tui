use crate::cli::DatabricksCli;
use crate::shape::{relative_time, ListItem, Shape, Status};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Max concurrent per-job run queries — keeps the number of `databricks`
/// subprocesses in flight bounded on large workspaces.
const CONCURRENCY: usize = 8;
/// Recent runs to pull per job: latest result + a short history strip.
const RUNS_PER_JOB: &str = "5";

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
    let detail = match (last_start.map(relative_time), next) {
        (Some(ago), Some(next)) => Some(format!("{ago} · {next}")),
        (ago, next) => ago.or(next),
    };
    ListItem {
        name,
        status,
        detail,
        id: job_id.map(|id| id.to_string()),
        history,
    }
}
