use crate::cli::DatabricksCli;
use crate::shape::{ListItem, Shape};
use anyhow::Result;

pub async fn fetch(cli: &DatabricksCli) -> Result<Shape> {
    let json = cli.run(&["pipelines", "list-pipelines"]).await?;
    let items = json
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|p| ListItem {
                    name: p["name"].as_str().unwrap_or("unknown").to_string(),
                    status: p["state"].as_str().unwrap_or("").parse().unwrap(),
                    detail: p["pipeline_id"].as_str().map(str::to_string),
                    id: p["pipeline_id"].as_str().map(str::to_string),
                    history: Vec::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Shape::List(items))
}
