use crate::cli::DatabricksCli;
use crate::fetchers::preview::run_sql;
use crate::shape::TableData;

const BUCKET_CASE: &str = "CASE \
    WHEN u.sku_name LIKE '%JOBS%' THEN 'Jobs' \
    WHEN u.sku_name LIKE '%DLT%' THEN 'DLT' \
    WHEN u.sku_name LIKE '%SQL%' THEN 'SQL' \
    WHEN u.sku_name LIKE '%ALL_PURPOSE%' THEN 'All-Purpose' \
    ELSE 'Other' END";

/// Attaches the list price that was in effect when the usage was billed.
const PRICE_JOIN: &str = "LEFT JOIN system.billing.list_prices lp \
    ON u.sku_name = lp.sku_name AND u.usage_unit = lp.usage_unit \
    AND u.usage_end_time >= lp.price_start_time \
    AND (lp.price_end_time IS NULL OR u.usage_end_time < lp.price_end_time)";

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
    /// True when usage is filtered to the current workspace; false
    /// means the whole account is shown (workspace id unresolved).
    pub scoped: bool,
}

/// `AND u.workspace_id = '<id>'` when the current workspace is known.
fn ws_clause(workspace_id: Option<&str>) -> String {
    match workspace_id {
        Some(id) => format!(" AND u.workspace_id = '{}'", id.replace('\'', "")),
        None => String::new(),
    }
}

fn priced_query(ws: &str) -> String {
    format!(
        "SELECT u.usage_date, {BUCKET_CASE} AS bucket, \
         ROUND(SUM(u.usage_quantity), 2) AS dbus, \
         ROUND(SUM(u.usage_quantity * COALESCE(lp.pricing.default, 0)), 2) AS usd \
         FROM system.billing.usage u {PRICE_JOIN} \
         WHERE u.usage_date >= date_sub(current_date(), 13){ws} \
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

fn spenders_query(priced: bool, ws: &str) -> String {
    let usd = if priced {
        ", ROUND(SUM(u.usage_quantity * COALESCE(lp.pricing.default, 0)), 2) AS usd"
    } else {
        ""
    };
    let join = if priced {
        format!("{PRICE_JOIN} ")
    } else {
        String::new()
    };
    // With prices, rank by dollars; DBUs are only a proxy without them.
    let order = if priced { "4" } else { "3" };
    format!(
        "SELECT {SPENDER_KIND} AS kind, {SPENDER_ID} AS id, \
         ROUND(SUM(u.usage_quantity), 2) AS dbus{usd} \
         FROM system.billing.usage u {join}\
         WHERE u.usage_date >= date_sub(current_date(), 13){ws} \
         GROUP BY 1, 2 ORDER BY {order} DESC LIMIT 10"
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

fn plain_query(ws: &str) -> String {
    format!(
        "SELECT u.usage_date, {BUCKET_CASE} AS bucket, \
         ROUND(SUM(u.usage_quantity), 2) AS dbus \
         FROM system.billing.usage u \
         WHERE u.usage_date >= date_sub(current_date(), 13){ws} \
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
        scoped: false,
    }
}

/// A pane item whose spend can be traced in system.billing.usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Job,
    Pipeline,
}

impl ResourceKind {
    pub fn label(self) -> &'static str {
        match self {
            ResourceKind::Job => "job",
            ResourceKind::Pipeline => "pipeline",
        }
    }

    /// The `usage_metadata` field usage is attributed through.
    fn column(self) -> &'static str {
        match self {
            ResourceKind::Job => "job_id",
            ResourceKind::Pipeline => "dlt_pipeline_id",
        }
    }
}

/// The rolling windows a resource's spend is reported over, as
/// (label, days). Ordered shortest first.
pub const WINDOWS: [(&str, i64); 4] = [
    ("last week", 7),
    ("last month", 30),
    ("last quarter", 90),
    ("last year", 365),
];

/// Spend over one rolling window ending today.
#[derive(Debug, Clone)]
pub struct CostWindow {
    pub label: &'static str,
    pub days: i64,
    pub dbus: f64,
    pub usd: f64,
    /// The same-length window immediately before this one, for a trend.
    /// None when that window predates what `usage` retains (365 days).
    pub prior: Option<(f64, f64)>,
}

impl CostWindow {
    /// Change against the prior window as a fraction, in dollars when
    /// priced and DBUs otherwise. None when there is nothing to compare
    /// against — no prior window, or a prior window of zero.
    pub fn trend(&self, priced: bool) -> Option<f64> {
        let (prior_dbus, prior_usd) = self.prior?;
        let (now, before) = if priced {
            (self.usd, prior_usd)
        } else {
            (self.dbus, prior_dbus)
        };
        if before <= 0.0 {
            return None;
        }
        Some((now - before) / before)
    }
}

/// A single job's or pipeline's spend over the last year.
#[derive(Debug, Clone)]
pub struct ResourceCost {
    pub kind: ResourceKind,
    pub windows: Vec<CostWindow>,
    /// Calendar-month totals as (YYYY-MM, dbus, usd), oldest first, at
    /// most the last 12 months that recorded usage.
    pub months: Vec<(String, f64, f64)>,
    /// False when list_prices was unreadable and only DBUs are shown.
    pub priced: bool,
    /// True when usage is filtered to the current workspace.
    pub scoped: bool,
}

impl ResourceCost {
    pub fn is_empty(&self) -> bool {
        self.months.is_empty()
    }
}

/// `AND u.usage_metadata.<field> = '<id>'` for the selected resource.
fn resource_clause(kind: ResourceKind, id: &str) -> String {
    format!(
        " AND u.usage_metadata.{} = '{}'",
        kind.column(),
        id.replace('\'', "")
    )
}

/// Daily usage for one resource over the last 365 days — the retention
/// of `system.billing.usage`. `days_ago` comes from the warehouse so the
/// windows are cut on the workspace's own clock, not the client's.
fn resource_query(priced: bool, kind: ResourceKind, id: &str, ws: &str) -> String {
    let usd = if priced {
        ", ROUND(SUM(u.usage_quantity * COALESCE(lp.pricing.default, 0)), 4) AS usd"
    } else {
        ""
    };
    let join = if priced {
        format!("{PRICE_JOIN} ")
    } else {
        String::new()
    };
    let resource = resource_clause(kind, id);
    format!(
        "SELECT u.usage_date, \
         CAST(datediff(current_date(), u.usage_date) AS INT) AS days_ago, \
         ROUND(SUM(u.usage_quantity), 4) AS dbus{usd} \
         FROM system.billing.usage u {join}\
         WHERE u.usage_date >= date_sub(current_date(), 364){resource}{ws} \
         GROUP BY 1, 2 ORDER BY 1"
    )
}

/// One day of a resource's usage: (days_ago, month, dbus, usd).
type ResourceDay = (i64, String, f64, f64);

fn parse_resource_days(table: &TableData) -> Vec<ResourceDay> {
    table
        .rows
        .iter()
        .filter_map(|row| {
            let (date, days_ago, dbus, usd) = match row.as_slice() {
                [d, a, v, u] => (d, a, v, Some(u)),
                [d, a, v] => (d, a, v, None),
                _ => return None,
            };
            let days_ago: i64 = days_ago.parse().ok()?;
            // "2026-07-01" -> "2026-07"
            let month: String = date.chars().take(7).collect();
            Some((
                days_ago,
                month,
                dbus.parse().unwrap_or(0.0),
                usd.map_or(0.0, |u| u.parse().unwrap_or(0.0)),
            ))
        })
        .collect()
}

fn aggregate_resource(days: &[ResourceDay], kind: ResourceKind, priced: bool) -> ResourceCost {
    let sum = |from: i64, to: i64| -> (f64, f64) {
        days.iter()
            .filter(|(ago, _, _, _)| *ago >= from && *ago < to)
            .fold((0.0, 0.0), |(d, u), (_, _, dbus, usd)| (d + dbus, u + usd))
    };
    let windows = WINDOWS
        .iter()
        .map(|(label, span)| {
            let (dbus, usd) = sum(0, *span);
            CostWindow {
                label,
                days: *span,
                dbus,
                usd,
                // Only compare against a window the table still covers.
                prior: (span * 2 <= 365).then(|| sum(*span, span * 2)),
            }
        })
        .collect();

    let mut months: Vec<(String, f64, f64)> = Vec::new();
    for (_, month, dbus, usd) in days {
        match months.last_mut() {
            Some((m, d, u)) if m == month => {
                *d += dbus;
                *u += usd;
            }
            _ => months.push((month.clone(), *dbus, *usd)),
        }
    }
    if months.len() > 12 {
        months.drain(..months.len() - 12);
    }

    ResourceCost {
        kind,
        windows,
        months,
        priced,
        scoped: false,
    }
}

/// Spend for a single job or pipeline: rolling week/month/quarter/year
/// totals with a prior-window trend, plus per-month totals for the year.
/// Usage is scoped to the given workspace when its id is known.
pub async fn fetch_resource(
    cli: &DatabricksCli,
    warehouse_id: &str,
    kind: ResourceKind,
    id: &str,
    workspace_id: Option<&str>,
) -> Result<ResourceCost, String> {
    let ws = ws_clause(workspace_id);
    let mut data = match run_sql(cli, &resource_query(true, kind, id, &ws), warehouse_id).await {
        Ok(table) => aggregate_resource(&parse_resource_days(&table), kind, true),
        // list_prices may be unreadable — fall back to DBUs only.
        Err(_) => {
            let sql = resource_query(false, kind, id, &ws);
            let table = run_sql(cli, &sql, warehouse_id).await?;
            aggregate_resource(&parse_resource_days(&table), kind, false)
        }
    };
    data.scoped = workspace_id.is_some();
    Ok(data)
}

/// Resolves the numeric id of the workspace behind `host` by matching
/// its URL in system.access.workspaces_latest. None when the table is
/// unreadable or the match is not unique.
pub async fn resolve_workspace_id(
    cli: &DatabricksCli,
    warehouse_id: &str,
    host: &str,
) -> Option<String> {
    let hostname = host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .replace('\'', "");
    let sql = format!(
        "SELECT CAST(workspace_id AS STRING) \
         FROM system.access.workspaces_latest \
         WHERE workspace_url LIKE '%{hostname}%' LIMIT 2"
    );
    let table = run_sql(cli, &sql, warehouse_id).await.ok()?;
    match table.rows.as_slice() {
        [row] => row.first().cloned(),
        _ => None,
    }
}

/// Daily DBU usage (and list-price dollar estimates when readable) for
/// the last 14 days from system.billing tables, plus the top resources
/// by DBU so spikes can be traced to a job/cluster/warehouse. With a
/// workspace id, usage is scoped to that workspace instead of the
/// whole account.
pub async fn fetch(
    cli: &DatabricksCli,
    warehouse_id: &str,
    workspace_id: Option<&str>,
) -> Result<CostData, String> {
    let ws = ws_clause(workspace_id);
    let mut data = match run_sql(cli, &priced_query(&ws), warehouse_id).await {
        Ok(table) => aggregate(&table, true),
        // list_prices may be unreadable — fall back to DBUs only.
        Err(_) => {
            let table = run_sql(cli, &plain_query(&ws), warehouse_id).await?;
            aggregate(&table, false)
        }
    };
    data.scoped = workspace_id.is_some();
    // Spenders are a bonus — a failure here shouldn't sink the whole view.
    if let Ok(table) = run_sql(cli, &spenders_query(data.priced, &ws), warehouse_id).await {
        data.spenders = parse_spenders(&table);
        if data.priced {
            data.spenders.sort_by(|a, b| {
                b.usd
                    .partial_cmp(&a.usd)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A day of usage `days_ago` days back, costing 1 DBU / $2.
    fn day(days_ago: i64, month: &str) -> ResourceDay {
        (days_ago, month.to_string(), 1.0, 2.0)
    }

    #[test]
    fn windows_are_cumulative_and_exclude_their_own_prior() {
        // One DBU a day for 60 days, today included.
        let days: Vec<ResourceDay> = (0..60).map(|d| day(d, "2026-07")).collect();
        let cost = aggregate_resource(&days, ResourceKind::Job, true);

        let week = &cost.windows[0];
        assert_eq!(week.days, 7);
        assert_eq!(week.dbus, 7.0);
        assert_eq!(week.usd, 14.0);
        // The 7 days before that, not the 7 counted above.
        assert_eq!(week.prior, Some((7.0, 14.0)));
        assert_eq!(week.trend(true), Some(0.0));

        assert_eq!(cost.windows[1].dbus, 30.0);
        assert_eq!(cost.windows[2].dbus, 60.0);
        assert_eq!(cost.windows[3].dbus, 60.0);
    }

    #[test]
    fn year_window_has_no_prior_to_compare_with() {
        // 365 days of retention can't cover the year before last.
        let cost = aggregate_resource(&[day(0, "2026-07")], ResourceKind::Job, true);
        let year = cost.windows.last().unwrap();
        assert_eq!(year.days, 365);
        assert_eq!(year.prior, None);
        assert_eq!(year.trend(true), None);
    }

    #[test]
    fn trend_is_none_when_the_prior_window_is_empty() {
        // Usage started three days ago: nothing to compare against.
        let days = vec![day(0, "2026-07"), day(1, "2026-07"), day(2, "2026-07")];
        let cost = aggregate_resource(&days, ResourceKind::Job, true);
        assert_eq!(cost.windows[0].prior, Some((0.0, 0.0)));
        assert_eq!(cost.windows[0].trend(true), None);
    }

    #[test]
    fn trend_follows_dbus_when_prices_are_unreadable() {
        let mut days: Vec<ResourceDay> = (0..7)
            .map(|d| (d, "2026-07".to_string(), 2.0, 0.0))
            .collect();
        days.extend((7..14).map(|d| (d, "2026-07".to_string(), 1.0, 0.0)));
        let cost = aggregate_resource(&days, ResourceKind::Job, false);
        // 14 DBU this week against 7 last week.
        assert_eq!(cost.windows[0].trend(false), Some(1.0));
    }

    #[test]
    fn months_roll_up_in_order_and_keep_the_last_twelve() {
        // 14 months of one day each, oldest first as the query returns them.
        let months: Vec<String> = (1..=12)
            .map(|m| format!("2025-{m:02}"))
            .chain((1..=2).map(|m| format!("2026-{m:02}")))
            .collect();
        let days: Vec<ResourceDay> = months
            .iter()
            .enumerate()
            .map(|(i, m)| day((13 - i as i64) * 30, m))
            .collect();
        let cost = aggregate_resource(&days, ResourceKind::Pipeline, true);
        assert_eq!(cost.months.len(), 12);
        // The two oldest months are dropped, and the rest stay in order.
        assert_eq!(cost.months.first().unwrap().0, "2025-03");
        assert_eq!(cost.months.last().unwrap().0, "2026-02");
    }

    #[test]
    fn same_month_days_are_summed_into_one_bar() {
        let days = vec![day(1, "2026-07"), day(2, "2026-07"), day(40, "2026-06")];
        let cost = aggregate_resource(&days, ResourceKind::Job, true);
        assert_eq!(cost.months.len(), 2);
        assert_eq!(cost.months[0], ("2026-07".to_string(), 2.0, 4.0));
    }

    #[test]
    fn resource_clause_targets_the_right_metadata_field() {
        assert!(resource_clause(ResourceKind::Job, "42").contains("usage_metadata.job_id = '42'"));
        assert!(resource_clause(ResourceKind::Pipeline, "abc")
            .contains("usage_metadata.dlt_pipeline_id = 'abc'"));
        // Quotes are stripped rather than escaped, as elsewhere here.
        assert_eq!(
            resource_clause(ResourceKind::Job, "4'2"),
            " AND u.usage_metadata.job_id = '42'"
        );
    }
}
