use crate::cli::DatabricksCli;
use crate::fetchers::preview::run_sql;

const QUERY: &str = "SELECT usage_date, CASE \
    WHEN sku_name LIKE '%JOBS%' THEN 'Jobs' \
    WHEN sku_name LIKE '%DLT%' THEN 'DLT' \
    WHEN sku_name LIKE '%SQL%' THEN 'SQL' \
    WHEN sku_name LIKE '%ALL_PURPOSE%' THEN 'All-Purpose' \
    ELSE 'Other' END AS bucket, \
    ROUND(SUM(usage_quantity), 2) AS dbus \
    FROM system.billing.usage \
    WHERE usage_date >= date_sub(current_date(), 13) \
    GROUP BY 1, 2 ORDER BY 1";

#[derive(Debug, Clone)]
pub struct CostDay {
    pub date: String,
    pub by_bucket: Vec<(String, f64)>,
    pub total: f64,
}

#[derive(Debug, Clone)]
pub struct CostData {
    pub days: Vec<CostDay>,
    /// Per-bucket totals over the window, largest first.
    pub buckets: Vec<(String, f64)>,
    pub total: f64,
}

/// Daily DBU usage for the last 14 days from system.billing.usage,
/// bucketed by SKU family.
pub async fn fetch(cli: &DatabricksCli, warehouse_id: &str) -> Result<CostData, String> {
    let table = run_sql(cli, QUERY, warehouse_id).await?;

    let mut days: Vec<CostDay> = Vec::new();
    let mut bucket_totals: Vec<(String, f64)> = Vec::new();
    for row in &table.rows {
        let [date, bucket, dbus] = row.as_slice() else {
            continue;
        };
        let value: f64 = dbus.parse().unwrap_or(0.0);
        if days.last().map(|d| &d.date) != Some(date) {
            days.push(CostDay {
                date: date.clone(),
                by_bucket: Vec::new(),
                total: 0.0,
            });
        }
        let day = days.last_mut().unwrap();
        day.by_bucket.push((bucket.clone(), value));
        day.total += value;
        match bucket_totals.iter_mut().find(|(b, _)| b == bucket) {
            Some((_, t)) => *t += value,
            None => bucket_totals.push((bucket.clone(), value)),
        }
    }
    bucket_totals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let total = bucket_totals.iter().map(|(_, t)| t).sum();

    Ok(CostData {
        days,
        buckets: bucket_totals,
        total,
    })
}
