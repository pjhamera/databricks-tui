use crate::cli::DatabricksCli;
use crate::shape::{ListItem, Shape};
use anyhow::Result;

pub async fn fetch(cli: &DatabricksCli) -> Result<Shape> {
    let json = cli.run(&["lakeview", "list"]).await?;
    // The CLI usually flattens paginated lists to a bare array; fall back
    // to the wrapped API shape just in case.
    let arr = json
        .as_array()
        .or_else(|| json["dashboards"].as_array())
        .cloned()
        .unwrap_or_default();
    let items = arr
        .iter()
        .map(|d| ListItem {
            name: d["display_name"].as_str().unwrap_or("unknown").to_string(),
            status: d["lifecycle_state"].as_str().unwrap_or("").parse().unwrap(),
            detail: d["update_time"].as_str().map(str::to_string),
            id: d["dashboard_id"].as_str().map(str::to_string),
            history: Vec::new(),
            alert: None,
        })
        .collect();
    Ok(Shape::List(items))
}
