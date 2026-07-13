use crate::cli::DatabricksCli;
use crate::shape::{ListItem, Shape, Status};
use anyhow::Result;
use serde_json::Value;

fn items_of(json: &Value) -> &[Value] {
    json.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn error_entry(what: &str, err: &anyhow::Error) -> ListItem {
    let msg = format!("{err:#}");
    let first_line = msg.lines().next().unwrap_or("error").to_string();
    ListItem {
        name: format!("{what} unavailable"),
        status: Status::Unknown("ERROR".to_string()),
        detail: Some(first_line),
        id: None,
        history: Vec::new(),
    }
}

fn entry(v: &Value, kind: &str) -> ListItem {
    ListItem {
        name: v["name"].as_str().unwrap_or("unknown").to_string(),
        status: Status::Unknown(kind.to_string()),
        detail: v["comment"].as_str().map(str::to_string),
        id: v["full_name"]
            .as_str()
            .or_else(|| v["name"].as_str())
            .map(str::to_string),
        history: Vec::new(),
    }
}

/// Lists one level of the Unity Catalog tree:
/// no path → catalogs, [catalog] → schemas, [catalog, schema] → tables,
/// views and volumes.
pub async fn fetch(cli: &DatabricksCli, path: &[String]) -> Result<Shape> {
    let mut items: Vec<ListItem> = match path {
        [] => items_of(&cli.run(&["catalogs", "list"]).await?)
            .iter()
            .map(|c| entry(c, "CATALOG"))
            .collect(),
        [catalog] => items_of(&cli.run(&["schemas", "list", catalog]).await?)
            .iter()
            .map(|s| entry(s, "SCHEMA"))
            .collect(),
        [catalog, schema, ..] => {
            let table_args = ["tables", "list", catalog, schema];
            let volume_args = ["volumes", "list", catalog, schema];
            let (tables, volumes) = tokio::join!(cli.run(&table_args), cli.run(&volume_args));
            // Surface failures as visible rows instead of silently dropping
            // a whole object kind from the listing.
            let mut items: Vec<ListItem> = match &tables {
                Ok(json) => items_of(json)
                    .iter()
                    .map(|t| {
                        let kind = match t["table_type"].as_str() {
                            Some("VIEW") | Some("MATERIALIZED_VIEW") => "VIEW",
                            _ => "TABLE",
                        };
                        entry(t, kind)
                    })
                    .collect(),
                Err(e) => vec![error_entry("tables", e)],
            };
            match &volumes {
                Ok(json) => items.extend(items_of(json).iter().map(|v| entry(v, "VOLUME"))),
                Err(e) => items.push(error_entry("volumes", e)),
            }
            items
        }
    };
    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(Shape::List(items))
}
