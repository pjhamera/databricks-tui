use crate::cli::DatabricksCli;
use crate::shape::{relative_time, ListItem, Shape, Status};
use anyhow::Result;
use serde_json::Value;

/// The CLI's `--output json` unwraps list responses to a bare array,
/// but the REST shape wraps them under a key — accept either.
fn rows<'a>(json: &'a Value, key: &str) -> &'a [Value] {
    if let Some(arr) = json.as_array() {
        arr
    } else {
        json[key].as_array().map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Lists secret scopes, or the keys inside one scope. Values are never
/// fetched or shown anywhere — by design.
pub async fn fetch(cli: &DatabricksCli, scope: Option<&str>) -> Result<Shape> {
    let mut items: Vec<ListItem> = match scope {
        None => {
            let json = cli.run(&["secrets", "list-scopes"]).await?;
            rows(&json, "scopes")
                .iter()
                .map(|s| {
                    let name = s["name"].as_str().unwrap_or("?").to_string();
                    ListItem {
                        id: Some(name.clone()),
                        name,
                        status: Status::Unknown("SCOPE".to_string()),
                        detail: s["backend_type"].as_str().map(str::to_string),
                        history: Vec::new(),
                        alert: None,
                    }
                })
                .collect()
        }
        Some(scope) => {
            let args = ["secrets", "list-secrets", scope];
            let json = cli.run(&args).await?;
            rows(&json, "secrets")
                .iter()
                .map(|k| {
                    let key = k["key"].as_str().unwrap_or("?").to_string();
                    ListItem {
                        id: Some(key.clone()),
                        name: key,
                        status: Status::Unknown("KEY".to_string()),
                        detail: k["last_updated_timestamp"]
                            .as_u64()
                            .map(|t| format!("updated {}", relative_time(t))),
                        history: Vec::new(),
                        alert: None,
                    }
                })
                .collect()
        }
    };
    items.sort_by_key(|i| i.name.to_lowercase());
    Ok(Shape::List(items))
}
