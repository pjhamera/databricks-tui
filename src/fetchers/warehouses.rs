use crate::cli::DatabricksCli;
use crate::shape::{ListItem, Shape};
use anyhow::Result;

pub async fn fetch(cli: &DatabricksCli) -> Result<Shape> {
    let json = cli.run(&["warehouses", "list"]).await?;
    let items = json
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|w| ListItem {
                    name: w["name"].as_str().unwrap_or("unknown").to_string(),
                    status: w["state"].as_str().unwrap_or("").parse().unwrap(),
                    detail: w["cluster_size"].as_str().map(str::to_string),
                    id: w["id"].as_str().map(str::to_string),
                    history: Vec::new(),
                    alert: None,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Shape::List(items))
}
