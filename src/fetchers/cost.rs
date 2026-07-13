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

/// One row of "who burned the DBUs": a job, cluster or warehouse.
#[derive(Debug, Clone)]
pub struct Spender {
    pub kind: String,
    pub id: String,
    pub dbus: f64,
    pub usd: f64,
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
    /// Top resources by DBU over the window, largest first.
    pub spenders: Vec<Spender>,
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

const SPENDER_KIND: &str = "CASE \
    WHEN u.usage_metadata.job_id IS NOT NULL THEN 'job' \
    WHEN u.usage_metadata.warehouse_id IS NOT NULL THEN 'warehouse' \
    WHEN u.usage_metadata.cluster_id IS NOT NULL THEN 'cluster' \
    ELSE 'other' END";

const SPENDER_ID: &str = "COALESCE(u.usage_metadata.job_id, \
    u.usage_metadata.warehouse_id, u.usage_metadata.cluster_id, u.sku_name)";

fn spenders_query(priced: bool) -> String {
    let usd = if priced {
        ", ROUND(SUM(u.usage_quantity * COALESCE(lp.pricing.default, 0)), 2) AS usd"
    } else {
        ""
    };
    let join = if priced {
        "LEFT JOIN system.billing.list_prices lp \
         ON u.sku_name = lp.sku_name AND u.usage_unit = lp.usage_unit \
         AND u.usage_end_time >= lp.price_start_time \
         AND (lp.price_end_time IS NULL OR u.usage_end_time < lp.price_end_time) "
    } else {
        ""
    };
    format!(
        "SELECT {SPENDER_KIND} AS kind, {SPENDER_ID} AS id, \
         ROUND(SUM(u.usage_quantity), 2) AS dbus{usd} \
         FROM system.billing.usage u {join}\
         WHERE u.usage_date >= date_sub(current_date(), 13) \
         GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 10"
    )
}

fn parse_spenders(table: &TableData) -> Vec<Spender> {
    table
        .rows
        .iter()
        .filter_map(|row| {
            let (kind, id, dbus, usd) = match row.as_slice() {
                [k, i, d, u] => (k, i, d.parse().ok()?, u.parse().unwrap_or(0.0)),
                [k, i, d] => (k, i, d.parse().ok()?, 0.0),
                _ => return None,
            };
            Some(Spender {
                kind: kind.clone(),
                id: id.clone(),
                dbus,
                usd,
            })
        })
        .collect()
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
        spenders: Vec::new(),
    }
}

/// Daily DBU usage (and list-price dollar estimates when readable) for
/// the last 14 days from system.billing tables, plus the top resources
/// by DBU so spikes can be traced to a job/cluster/warehouse.
pub async fn fetch(cli: &DatabricksCli, warehouse_id: &str) -> Result<CostData, String> {
    let mut data = match run_sql(cli, &priced_query(), warehouse_id).await {
        Ok(table) => aggregate(&table, true),
        // list_prices may be unreadable — fall back to DBUs only.
        Err(_) => {
            let table = run_sql(cli, &plain_query(), warehouse_id).await?;
            aggregate(&table, false)
        }
    };
    // Spenders are a bonus — a failure here shouldn't sink the whole view.
    if let Ok(table) = run_sql(cli, &spenders_query(data.priced), warehouse_id).await {
        data.spenders = parse_spenders(&table);
    }
    Ok(data)
}
