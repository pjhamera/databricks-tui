use crate::cli::DatabricksCli;
use crate::shape::TableData;
use serde_json::Value;
use std::time::Duration;

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

/// Runs `SELECT * LIMIT 50` on the given warehouse via the Statement
/// Execution API and shapes the result for the table renderer.
pub async fn fetch(
    cli: &DatabricksCli,
    full_name: &str,
    warehouse_id: &str,
) -> Result<TableData, String> {
    let payload = serde_json::json!({
        "statement": format!("SELECT * FROM {} LIMIT 50", quoted(full_name)),
        "warehouse_id": warehouse_id,
        "wait_timeout": "30s",
        "disposition": "INLINE",
        "format": "JSON_ARRAY",
    })
    .to_string();

    let mut resp = cli
        .run(&["api", "post", "/api/2.0/sql/statements", "--json", &payload])
        .await
        .map_err(|e| format!("{e:#}"))?;

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
            _ => return Err(error_of(&resp)),
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
