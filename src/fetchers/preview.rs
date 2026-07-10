use crate::cli::DatabricksCli;
use crate::shape::TableData;
use serde_json::Value;
use std::time::Duration;

/// Enriches a statement failure with what the workspace itself says
/// about the warehouse, separating permission problems from stale ids.
async fn diagnose(cli: &DatabricksCli, warehouse_id: &str, base_err: String) -> String {
    match cli.run(&["warehouses", "get", warehouse_id]).await {
        Ok(j) => {
            let state = j["state"].as_str().unwrap_or("unknown");
            let wtype = j["warehouse_type"].as_str().unwrap_or("unknown");
            let serverless = j["enable_serverless_compute"]
                .as_bool()
                .map(|b| b.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "{base_err}\n\ndiagnostic: warehouse {warehouse_id} DOES exist in this \
                 workspace (state: {state}, type: {wtype}, serverless: {serverless}), yet \
                 the SQL Statements API rejected it — check CAN USE permission, or try \
                 running the same query with the CLI directly:\n  databricks api post \
                 /api/2.0/sql/statements --json \
                 '{{\"statement\":\"SELECT 1\",\"warehouse_id\":\"{warehouse_id}\"}}'"
            )
        }
        Err(e) => format!(
            "{base_err}\n\ndiagnostic: `warehouses get {warehouse_id}` also fails \
             ({e:#}) — the warehouse does not exist in the workspace this query went to \
             (stale list entry, or the query was routed to a different workspace)"
        ),
    }
}

fn quoted(full_name: &str) -> String {
    full_name
        .split('.')
        .map(|part| format!("`{part}`"))
        .collect::<Vec<_>>()
        .join(".")
}

fn state_of(resp: &Value) -> &str {
    resp["status"]["state"].as_str().unwrap_or("")
}

fn error_of(resp: &Value) -> String {
    resp["status"]["error"]["message"]
        .as_str()
        .unwrap_or("statement failed")
        .to_string()
}

/// Runs `SELECT * LIMIT 50` on the given warehouse.
pub async fn fetch(
    cli: &DatabricksCli,
    full_name: &str,
    warehouse_id: &str,
) -> Result<TableData, String> {
    let sql = format!("SELECT * FROM {} LIMIT 50", quoted(full_name));
    run_sql(cli, &sql, warehouse_id).await
}

/// Runs arbitrary SQL on the given warehouse via the Statement Execution
/// API and shapes the result as a table.
pub async fn run_sql(
    cli: &DatabricksCli,
    sql: &str,
    warehouse_id: &str,
) -> Result<TableData, String> {
    let payload = serde_json::json!({
        "statement": sql,
        "warehouse_id": warehouse_id,
        "wait_timeout": "30s",
        "disposition": "INLINE",
        "format": "JSON_ARRAY",
    })
    .to_string();

    let mut resp = match cli
        .run(&["api", "post", "/api/2.0/sql/statements", "--json", &payload])
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(diagnose(cli, warehouse_id, format!("{e:#}")).await),
    };

    // The warehouse may need minutes to cold-start; keep polling the statement.
    let id = resp["statement_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    for _ in 0..45 {
        match state_of(&resp) {
            "SUCCEEDED" => break,
            "PENDING" | "RUNNING" => {
                tokio::time::sleep(Duration::from_secs(4)).await;
                let path = format!("/api/2.0/sql/statements/{id}");
                resp = cli
                    .run(&["api", "get", &path])
                    .await
                    .map_err(|e| format!("{e:#}"))?;
            }
            _ => return Err(diagnose(cli, warehouse_id, error_of(&resp)).await),
        }
    }
    if state_of(&resp) != "SUCCEEDED" {
        // Don't leave the query queued against the warehouse.
        let cancel = format!("/api/2.0/sql/statements/{id}/cancel");
        let _ = cli.run_action(&["api", "post", &cancel]).await;
        return Err(
            "query did not finish in 3 minutes — check the warehouse in the SQL Warehouses pane"
                .to_string(),
        );
    }

    let headers: Vec<String> = resp["manifest"]["schema"]["columns"]
        .as_array()
        .map(|cols| {
            cols.iter()
                .map(|c| c["name"].as_str().unwrap_or("?").to_string())
                .collect()
        })
        .unwrap_or_default();
    let rows: Vec<Vec<String>> = resp["result"]["data_array"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    row.as_array()
                        .map(|cells| {
                            cells
                                .iter()
                                .map(|c| match c {
                                    Value::Null => "␀".to_string(),
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    if headers.is_empty() {
        return Err("no result schema returned".to_string());
    }
    Ok(TableData { headers, rows })
}
