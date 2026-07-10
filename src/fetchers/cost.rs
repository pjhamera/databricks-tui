use crate::cli::DatabricksCli;
use crate::fetchers::preview::run_sql;
use crate::shape::TableData;

const BUCKET_CASE: &str = "CASE \
    WHEN u.sku_name LIKE '%JOBS%' THEN 'Jobs' \
    WHEN u.sku_name LIKE '%DLT%' THEN 'DLT' \
    WHEN u.sku_name LIKE '%SQL%' THEN 'SQL' \
    WHEN u.sku_name LIKE '%ALL_PURPOSE%' THEN 'All-Purpose' \
    ELSE 'Other' END";

#[derive(Debug, Clone)]
pub struct CostDay {
    pub date: String,
    pub by_bucket: Vec<(String, f64)>,
    pub total: f64,
    pub total_usd: f64,
}

#[derive(Debug, Clone)]
pub struct CostData {
    pub days: Vec<CostDay>,
    /// Per-bucket (name, dbus, usd) totals over the window, largest first.
    pub buckets: Vec<(String, f64, f64)>,
    pub total: f64,
    pub total_usd: f64,
    /// False when list_prices was unreadable and only DBUs are shown.
    pub priced: bool,
}

fn priced_query() -> String {
    format!(
        "SELECT u.usage_date, {BUCKET_CASE} AS bucket, \
         ROUND(SUM(u.usage_quantity), 2) AS dbus, \
         ROUND(SUM(u.usage_quantity * COALESCE(lp.pricing.default, 0)), 2) AS usd \
         FROM system.billing.usage u \
         LEFT JOIN system.billing.list_prices lp \
           ON u.sku_name = lp.sku_name AND u.usage_unit = lp.usage_unit \
           AND u.usage_end_time >= lp.price_start_time \
           AND (lp.price_end_time IS NULL OR u.usage_end_time < lp.price_end_time) \
         WHERE u.usage_date >= date_sub(current_date(), 13) \
         GROUP BY 1, 2 ORDER BY 1"
    )
}

fn plain_query() -> String {
    format!(
        "SELECT u.usage_date, {BUCKET_CASE} AS bucket, \
         ROUND(SUM(u.usage_quantity), 2) AS dbus \
         FROM system.billing.usage u \
         WHERE u.usage_date >= date_sub(current_date(), 13) \
         GROUP BY 1, 2 ORDER BY 1"
    )
}

fn aggregate(table: &TableData, priced: bool) -> CostData {
    let mut days: Vec<CostDay> = Vec::new();
    let mut bucket_totals: Vec<(String, f64, f64)> = Vec::new();
    for row in &table.rows {
        let (date, bucket, dbus, usd) = match row.as_slice() {
            [d, b, v, u] => (d, b, v.parse().unwrap_or(0.0), u.parse().unwrap_or(0.0)),
            [d, b, v] => (d, b, v.parse().unwrap_or(0.0), 0.0),
            _ => continue,
        };
        if days.last().map(|d| &d.date) != Some(date) {
            days.push(CostDay {
                date: date.clone(),
                by_bucket: Vec::new(),
                total: 0.0,
                total_usd: 0.0,
            });
        }
        let day = days.last_mut().unwrap();
        day.by_bucket.push((bucket.clone(), dbus));
        day.total += dbus;
        day.total_usd += usd;
        match bucket_totals.iter_mut().find(|(b, _, _)| b == bucket) {
            Some((_, t, tu)) => {
                *t += dbus;
                *tu += usd;
            }
            None => bucket_totals.push((bucket.clone(), dbus, usd)),
        }
    }
    bucket_totals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let total = bucket_totals.iter().map(|(_, t, _)| t).sum();
    let total_usd = bucket_totals.iter().map(|(_, _, u)| u).sum();

    CostData {
        days,
        buckets: bucket_totals,
        total,
        total_usd,
        priced,
    }
}

/// Daily DBU usage (and list-price dollar estimates when readable) for
/// the last 14 days from system.billing tables.
pub async fn fetch(cli: &DatabricksCli, warehouse_id: &str) -> Result<CostData, String> {
    match run_sql(cli, &priced_query(), warehouse_id).await {
        Ok(table) => Ok(aggregate(&table, true)),
        // list_prices may be unreadable — fall back to DBUs only.
        Err(_) => {
            let table = run_sql(cli, &plain_query(), warehouse_id).await?;
            Ok(aggregate(&table, false))
        }
    }
}
