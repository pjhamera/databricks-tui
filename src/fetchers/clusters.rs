use crate::cli::DatabricksCli;
use crate::shape::{ListItem, Shape};
use anyhow::Result;

pub async fn fetch(cli: &DatabricksCli) -> Result<Shape> {
    // Without a source filter the API also returns every job-created cluster
    // terminated in the last 30 days, which can be hundreds of entries.
    let json = cli
        .run(&[
            "clusters",
            "list",
            "--cluster-sources",
            "UI",
            "--cluster-sources",
            "API",
        ])
        .await?;
    let items = json
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| ListItem {
                    name: c["cluster_name"].as_str().unwrap_or("unknown").to_string(),
                    status: c["state"].as_str().unwrap_or("").parse().unwrap(),
                    detail: c["cluster_id"].as_str().map(str::to_string),
                    id: c["cluster_id"].as_str().map(str::to_string),
                    history: Vec::new(),
                    alert: None,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Shape::List(items))
}
