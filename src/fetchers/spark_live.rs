use crate::cli::DatabricksCli;
use serde_json::Value;

/// Org id `0` and driver UI port `40001` are a community-documented
/// convention for reaching a cluster's Spark UI through Databricks'
/// driver-proxy path — this is NOT a documented public API and may not
/// hold on every cloud/workspace. Despite the name, this same path also
/// appears to serve cached results for runs whose cluster has already
/// terminated (confirmed against a live workspace: the Databricks web
/// UI keeps showing Spark UI for several previous runs, not just the
/// current one), so this module no longer gates on the run still being
/// "live" — it just attempts the call and lets it fail cleanly if the
/// data is gone. Exactly how long results stay reachable isn't
/// documented; a single attempt that fails cleanly is the intended
/// scope, not a fallback/scanning loop.
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

#[derive(Debug, Clone)]
pub struct SparkLiveData {
    pub app_id: String,
    pub run_id: String,
    /// Most recent stages, newest first.
    pub stages: Vec<StageDiag>,
}

/// Newest run of the job, straight from the API (not shared with
/// `RunView`'s state, keeping this module fully decoupled).
async fn discover_run(cli: &DatabricksCli, job_id: &str) -> Result<Value, String> {
    let args = ["jobs", "list-runs", "--job-id", job_id, "--limit", "1"];
    let json = cli.run(&args).await.map_err(|e| format!("{e:#}"))?;
    json.as_array()
        .cloned()
        .or_else(|| json["runs"].as_array().cloned())
        .and_then(|runs| runs.into_iter().next())
        .ok_or_else(|| "no runs found for this job".to_string())
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

async fn list_apps(cli: &DatabricksCli, cluster_id: &str) -> Result<String, String> {
    let path = format!(
        "/driver-proxy-api/o/{DRIVER_PROXY_ORG}/{cluster_id}/{DRIVER_PROXY_PORT}/api/v1/applications"
    );
    let json = cli
        .run(&["api", "get", &path])
        .await
        .map_err(|e| format!("{e:#}"))?;
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
    let json = cli
        .run(&["api", "get", &path])
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(parse_stages(&json))
}

/// Per-stage skew and spill from Spark's own `stages?details=true`
/// response. Most recent ~10 stages, newest first; 0/1-task stages get
/// a 0.0 skew ratio rather than a misleading number.
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
            durations.sort_unstable();
            let max_task_duration_ms = durations.last().copied().unwrap_or(0);
            let median_task_duration_ms = if durations.is_empty() {
                0
            } else {
                durations[durations.len() / 2]
            };
            let skew_ratio = if durations.len() >= 2 && median_task_duration_ms > 0 {
                max_task_duration_ms as f64 / median_task_duration_ms as f64
            } else {
                0.0
            };

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

/// Best-effort spill/skew diagnostics for the job's most recent run, via
/// the undocumented driver-proxy path in front of that run's Spark UI.
/// Every failure path returns a short, UI-safe `Err` — never panics —
/// so the caller can show it as a calm "unavailable" note rather than
/// an error; there's no upfront eligibility check, since the same path
/// appears to keep serving results for a while after the cluster is
/// gone and the exact window isn't documented — the call is simply
/// attempted and allowed to fail cleanly.
pub async fn fetch(cli: &DatabricksCli, job_id: &str) -> Result<SparkLiveData, String> {
    let run = discover_run(cli, job_id).await?;
    let run_id = run["run_id"]
        .as_u64()
        .ok_or_else(|| "run has no run_id".to_string())?
        .to_string();
    let cluster_id = discover_cluster(cli, &run_id).await?;
    let app_id = list_apps(cli, &cluster_id).await?;
    let stages = fetch_stages(cli, &cluster_id, &app_id).await?;
    Ok(SparkLiveData {
        app_id,
        run_id,
        stages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
