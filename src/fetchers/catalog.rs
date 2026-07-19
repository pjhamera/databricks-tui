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
        alert: None,
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
        alert: None,
    }
}

const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "csv", "tsv", "json", "jsonl", "ndjson", "md", "log", "yaml", "yml", "xml", "sql", "py",
    "sh", "conf", "ini", "toml", "html",
];

/// Shows the head of a text-ish file inside a volume via `fs cat`.
pub async fn file_peek(cli: &DatabricksCli, path: &str) -> crate::shape::DetailData {
    let textish = path
        .rsplit('.')
        .next()
        .is_some_and(|ext| TEXT_EXTENSIONS.contains(&ext.to_lowercase().as_str()));
    let raw = if !textish {
        format!(
            "{path}\n\nno preview for this file type — download it with:\n  databricks fs cp {path} ."
        )
    } else {
        match cli.run_raw(&["fs", "cat", path]).await {
            Ok(content) => {
                let total_lines = content.lines().count();
                let head: String = content
                    .lines()
                    .take(200)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .chars()
                    .take(64 * 1024)
                    .collect();
                let note = if total_lines > 200 {
                    format!("\n\n… first 200 of {total_lines} lines")
                } else {
                    String::new()
                };
                format!("{path}\n\n{head}{note}")
            }
            Err(e) => format!("{path}\n\n✗ {e:#}"),
        }
    };
    crate::shape::DetailData {
        summary: Vec::new(),
        activity: Vec::new(),
        raw,
    }
}

fn fmt_size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.1} KB", bytes as f64 / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
        _ => format!("{:.1} GB", bytes as f64 / 1_073_741_824.0),
    }
}

/// Bare object names at one level of the Unity Catalog tree, for SQL
/// completion: "" → catalogs, "cat" → schemas, "cat.sch" → tables and
/// views, "cat.sch.tab" → columns.
pub async fn names(cli: &DatabricksCli, path: &str) -> Result<Vec<String>> {
    let parts: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    let json = match parts.as_slice() {
        [] => cli.run(&["catalogs", "list"]).await?,
        [catalog] => cli.run(&["schemas", "list", catalog]).await?,
        [catalog, schema] => cli.run(&["tables", "list", catalog, schema]).await?,
        _ => {
            let table = cli.run(&["tables", "get", path]).await?;
            return Ok(items_of(&table["columns"])
                .iter()
                .filter_map(|c| c["name"].as_str().map(str::to_string))
                .collect());
        }
    };
    let mut names: Vec<String> = items_of(&json)
        .iter()
        .filter_map(|v| v["name"].as_str().map(str::to_string))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    Ok(names)
}

/// Lists one level of the Unity Catalog tree:
/// no path → catalogs, [catalog] → schemas, [catalog, schema] → tables,
/// views and volumes, deeper → files inside a volume.
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
        [catalog, schema, rest @ ..] if !rest.is_empty() => {
            let vol_path = format!("dbfs:/Volumes/{catalog}/{schema}/{}", rest.join("/"));
            let args = ["fs", "ls", &vol_path];
            items_of(&cli.run(&args).await?)
                .iter()
                .map(|f| {
                    // `fs ls --output json` gives basename `name`, `is_directory`,
                    // `size`, and `last_modified` (ISO 8601) — no `path`. Older
                    // shapes used is_dir/file_size/modification_time; accept both.
                    let name = f["name"]
                        .as_str()
                        .or_else(|| f["path"].as_str())
                        .unwrap_or("?")
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("?")
                        .to_string();
                    let is_dir = f["is_directory"]
                        .as_bool()
                        .or_else(|| f["is_dir"].as_bool())
                        .unwrap_or(false);
                    let detail = if is_dir {
                        None
                    } else {
                        let size = f["size"]
                            .as_u64()
                            .or_else(|| f["file_size"].as_u64())
                            .map(fmt_size)
                            .unwrap_or_default();
                        // last_modified is an ISO string; show its date part.
                        let when = f["last_modified"]
                            .as_str()
                            .filter(|s| !s.starts_with("1970"))
                            .map(|s| s.chars().take(10).collect::<String>())
                            .or_else(|| {
                                f["modification_time"]
                                    .as_u64()
                                    .filter(|t| *t > 0)
                                    .map(crate::shape::relative_time)
                            })
                            .unwrap_or_default();
                        Some(format!("{size}  {when}").trim().to_string())
                    };
                    ListItem {
                        // Reconstruct the full path — the detail view `fs cat`s it.
                        id: (!is_dir).then(|| format!("{vol_path}/{name}")),
                        status: Status::Unknown(if is_dir { "DIR" } else { "FILE" }.to_string()),
                        detail,
                        name,
                        history: Vec::new(),
                        alert: None,
                    }
                })
                .collect()
        }
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
    items.sort_by_key(|i| i.name.to_lowercase());
    Ok(Shape::List(items))
}
