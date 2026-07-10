use crate::cli::DatabricksCli;
use crate::fetchers::preview::run_sql;
use crate::shape::{DetailData, Status};

/// Upstream and downstream tables for one table, from
/// system.access.table_lineage (last 90 days of lineage events).
pub async fn fetch(cli: &DatabricksCli, full_name: &str, warehouse_id: &str) -> DetailData {
    let fq = full_name.replace('\'', "''");
    let sql = format!(
        "SELECT 'upstream' AS dir, source_table_full_name AS name \
         FROM system.access.table_lineage \
         WHERE target_table_full_name = '{fq}' \
           AND source_table_full_name IS NOT NULL \
           AND event_time >= date_sub(current_date(), 90) \
         GROUP BY 1, 2 \
         UNION ALL \
         SELECT 'downstream', target_table_full_name \
         FROM system.access.table_lineage \
         WHERE source_table_full_name = '{fq}' \
           AND target_table_full_name IS NOT NULL \
           AND event_time >= date_sub(current_date(), 90) \
         GROUP BY 1, 2 \
         LIMIT 200"
    );

    let table = match run_sql(cli, &sql, warehouse_id).await {
        Ok(t) => t,
        Err(e) => {
            return DetailData {
                summary: Vec::new(),
                activity: Vec::new(),
                raw: format!("{e}\n\nlineage needs read access to system.access.table_lineage"),
            }
        }
    };

    let mut upstream: Vec<String> = Vec::new();
    let mut downstream: Vec<String> = Vec::new();
    for row in &table.rows {
        if let [dir, name] = row.as_slice() {
            match dir.as_str() {
                "upstream" => upstream.push(name.clone()),
                _ => downstream.push(name.clone()),
            }
        }
    }
    upstream.sort();
    downstream.sort();

    let summary = vec![
        ("Object".to_string(), full_name.to_string()),
        ("Upstream".to_string(), upstream.len().to_string()),
        ("Downstream".to_string(), downstream.len().to_string()),
        ("Window".to_string(), "last 90 days".to_string()),
    ];
    let mut activity: Vec<(Status, String)> = Vec::new();
    for name in &upstream {
        activity.push((Status::Success, format!("▲ {name}")));
    }
    for name in &downstream {
        activity.push((Status::Pending, format!("▼ {name}")));
    }
    if activity.is_empty() {
        activity.push((
            Status::Unknown(String::new()),
            "no lineage events recorded in the window".to_string(),
        ));
    }
    let raw = table
        .rows
        .iter()
        .map(|r| r.join(" · "))
        .collect::<Vec<_>>()
        .join("\n");

    DetailData {
        summary,
        activity,
        raw,
    }
}
