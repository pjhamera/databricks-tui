use crate::cli::DatabricksCli;
use crate::shape::{relative_time, ListItem, Shape, Status};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

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

pub async fn fetch(cli: &DatabricksCli) -> Result<Shape> {
    // Jobs alone carry no health signal — join them with recent runs so each
    // job shows its latest result and a short run history.
    let (jobs, runs) = tokio::join!(
        cli.run(&["jobs", "list"]),
        cli.run(&["jobs", "list-runs", "--limit", "25"]),
    );
    let jobs = jobs?;
    let runs = runs.unwrap_or(Value::Null);

    // list-runs is newest-first; group per job preserving that order.
    let mut runs_by_job: HashMap<u64, Vec<&Value>> = HashMap::new();
    if let Some(arr) = runs.as_array() {
        for r in arr {
            if let Some(job_id) = r["job_id"].as_u64() {
                runs_by_job.entry(job_id).or_default().push(r);
            }
        }
    }

    let items = jobs
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|j| {
                    let job_id = j["job_id"].as_u64();
                    let job_runs = job_id
                        .and_then(|id| runs_by_job.get(&id))
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let status = job_runs
                        .first()
                        .map(|r| run_status(r))
                        .unwrap_or(Status::Unknown("NO RUNS".to_string()));
                    let history: Vec<Status> = job_runs
                        .iter()
                        .take(5)
                        .rev()
                        .map(|r| run_status(r))
                        .collect();
                    let detail = job_runs
                        .first()
                        .and_then(|r| r["start_time"].as_u64())
                        .map(relative_time);
                    ListItem {
                        name: j["settings"]["name"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string(),
                        status,
                        detail,
                        id: job_id.map(|id| id.to_string()),
                        history,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Shape::List(items))
}
