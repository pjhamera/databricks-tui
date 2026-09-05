//! Dataset usage and staleness.
//!
//! Answers "is anyone actually reading this?" for Unity Catalog tables,
//! by pairing two system sources that only mean something together:
//!
//! - `system.access.table_lineage` records one row per read or write
//!   event, so `MAX(event_time)` over rows where a table is the *source*
//!   is its last read. Retention is a rolling year.
//! - `<catalog>.information_schema.tables` lists what actually exists.
//!
//! The join has to be a LEFT JOIN from the catalog side, and that is the
//! whole point of this module: a table nobody has touched since before
//! the lineage window has *no rows at all* in `table_lineage`. Query
//! lineage alone and the deadest tables in the workspace are invisible
//! rather than obvious.

use crate::cli::DatabricksCli;
use crate::fetchers::preview::run_sql;

/// Default age, in days, past which a table is called stale. Overridable
/// per user via `stale_days` in the config file.
pub const DEFAULT_STALE_DAYS: i64 = 90;

/// How far back the read/write counts are totalled. The lineage table
/// keeps a rolling year, so this is the most the source can honestly
/// support.
pub const WINDOW_DAYS: i64 = 365;

/// Most tables one scan will report on. A catalog can hold thousands;
/// the report ranks by staleness first, so the cap drops the *freshest*
/// rows — the ones nobody is asking about.
pub const MAX_ROWS: usize = 500;

/// How stale one table is, as a coarse band the UI colors by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Read within the threshold.
    Active,
    /// No read in `stale_days`, but there was one inside the window.
    Stale,
    /// No read event at all in the lineage window.
    NeverRead,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableUsage {
    pub full_name: String,
    pub schema: String,
    /// MANAGED, EXTERNAL, VIEW, …, straight from information_schema.
    pub table_type: String,
    /// Days since the last read event; None when never read in window.
    pub days_since_read: Option<i64>,
    /// Days since the last write event; None when never written in window.
    pub days_since_write: Option<i64>,
    /// Read events in the window.
    pub reads: i64,
    /// Distinct principals that read it in the window.
    pub readers: i64,
    /// Days since the table was created.
    pub age_days: Option<i64>,
    pub freshness: Freshness,
}

impl TableUsage {
    /// The one-line reason this table is on the report, used both as the
    /// pane `alert` text and in the report body.
    pub fn note(&self) -> String {
        match (self.freshness, self.days_since_read) {
            (Freshness::NeverRead, _) => match self.age_days {
                // A table created yesterday being unread is not news.
                Some(age) => format!("no reads in {WINDOW_DAYS}d · created {age}d ago"),
                None => format!("no reads in {WINDOW_DAYS}d"),
            },
            (Freshness::Stale, Some(d)) => format!("no reads in {d}d"),
            (Freshness::Stale, None) => format!("no reads in {WINDOW_DAYS}d"),
            (Freshness::Active, Some(d)) => format!("last read {d}d ago"),
            (Freshness::Active, None) => "read recently".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageScan {
    /// What was scanned: a catalog name, or `catalog.schema`.
    pub scope: String,
    pub stale_days: i64,
    /// Every table in scope, most stale first.
    pub tables: Vec<TableUsage>,
    /// Tables in scope before `MAX_ROWS` truncation.
    pub total: usize,
    pub stale_count: usize,
    pub never_read_count: usize,
}

impl UsageScan {
    /// Tables at or past the threshold, i.e. what the alert counts.
    pub fn flagged(&self) -> impl Iterator<Item = &TableUsage> {
        self.tables
            .iter()
            .filter(|t| t.freshness != Freshness::Active)
    }
}

/// Escapes a SQL string literal. The system-table queries elsewhere in
/// this codebase strip quotes rather than escape them; doubling is the
/// safer form and matches `lineage::hop`.
fn lit(s: &str) -> String {
    s.replace('\'', "''")
}

/// The scan query.
///
/// `information_schema.tables` is per-catalog and drives the LEFT JOIN so
/// that never-read tables survive it. The lineage side is pre-aggregated
/// in a CTE rather than joined raw, so the join is one row per table
/// instead of one per event.
///
/// Note `source_table_full_name` for reads: a row where the table is the
/// *target* is a write, which says nothing about whether anyone consumes
/// the data. Writes are counted separately, as a pipeline still filling a
/// table nobody reads is exactly the waste worth reporting.
fn scan_query(catalog: &str, schema: Option<&str>) -> String {
    let cat = lit(catalog);
    let schema_pred = match schema {
        Some(s) => format!(" AND t.table_schema = '{}'", lit(s)),
        None => String::new(),
    };
    // Lineage is filtered by catalog on both sides so the CTEs prune to
    // the same slice the information_schema side covers.
    format!(
        "WITH reads AS ( \
           SELECT source_table_full_name AS full_name, \
                  MAX(event_time) AS last_event, \
                  COUNT(*) AS events, \
                  COUNT(DISTINCT created_by) AS principals \
           FROM system.access.table_lineage \
           WHERE source_table_catalog = '{cat}' \
             AND source_table_full_name IS NOT NULL \
             AND event_time >= date_sub(current_timestamp(), {WINDOW_DAYS}) \
           GROUP BY 1 \
         ), writes AS ( \
           SELECT target_table_full_name AS full_name, \
                  MAX(event_time) AS last_event \
           FROM system.access.table_lineage \
           WHERE target_table_catalog = '{cat}' \
             AND target_table_full_name IS NOT NULL \
             AND event_time >= date_sub(current_timestamp(), {WINDOW_DAYS}) \
           GROUP BY 1 \
         ) \
         SELECT CONCAT_WS('.', t.table_catalog, t.table_schema, t.table_name) AS full_name, \
                t.table_schema, \
                t.table_type, \
                CAST(datediff(current_timestamp(), r.last_event) AS INT) AS days_since_read, \
                CAST(datediff(current_timestamp(), w.last_event) AS INT) AS days_since_write, \
                COALESCE(r.events, 0) AS reads, \
                COALESCE(r.principals, 0) AS readers, \
                CAST(datediff(current_timestamp(), t.created) AS INT) AS age_days \
         FROM {catalog_ref}.information_schema.tables t \
         LEFT JOIN reads r \
           ON r.full_name = CONCAT_WS('.', t.table_catalog, t.table_schema, t.table_name) \
         LEFT JOIN writes w \
           ON w.full_name = CONCAT_WS('.', t.table_catalog, t.table_schema, t.table_name) \
         WHERE t.table_schema <> 'information_schema'{schema_pred} \
         ORDER BY CASE WHEN r.last_event IS NULL THEN 1 ELSE 0 END DESC, \
                  days_since_read DESC NULLS FIRST, \
                  reads ASC \
         LIMIT {limit}",
        catalog_ref = quoted_ident(catalog),
        limit = MAX_ROWS + 1,
    )
}

/// Backtick-quotes one identifier so a catalog named e.g. `my-catalog`
/// still parses.
fn quoted_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// Parses one scan row. Cells arrive as strings, with NULL as "␀" — the
/// sentinel `preview::run_sql` substitutes.
fn parse_row(row: &[String], stale_days: i64) -> Option<TableUsage> {
    let num = |cell: &String| -> Option<i64> {
        if cell == "␀" {
            None
        } else {
            cell.parse::<i64>().ok()
        }
    };
    let [full_name, schema, table_type, dsr, dsw, reads, readers, age] = row else {
        return None;
    };
    let days_since_read = num(dsr);
    let freshness = match days_since_read {
        None => Freshness::NeverRead,
        Some(d) if d >= stale_days => Freshness::Stale,
        Some(_) => Freshness::Active,
    };
    Some(TableUsage {
        full_name: full_name.clone(),
        schema: schema.clone(),
        table_type: table_type.clone(),
        days_since_read,
        days_since_write: num(dsw),
        reads: num(reads).unwrap_or(0),
        readers: num(readers).unwrap_or(0),
        age_days: num(age),
        freshness,
    })
}

/// Scans one catalog — or one schema inside it — for unread tables.
pub async fn scan(
    cli: &DatabricksCli,
    warehouse_id: &str,
    catalog: &str,
    schema: Option<&str>,
    stale_days: i64,
) -> Result<UsageScan, String> {
    let sql = scan_query(catalog, schema);
    let table = run_sql(cli, &sql, warehouse_id).await?;

    let mut tables: Vec<TableUsage> = table
        .rows
        .iter()
        .filter_map(|r| parse_row(r, stale_days))
        .collect();
    let total = tables.len();
    tables.truncate(MAX_ROWS);

    let stale_count = tables
        .iter()
        .filter(|t| t.freshness == Freshness::Stale)
        .count();
    let never_read_count = tables
        .iter()
        .filter(|t| t.freshness == Freshness::NeverRead)
        .count();

    Ok(UsageScan {
        scope: match schema {
            Some(s) => format!("{catalog}.{s}"),
            None => catalog.to_string(),
        },
        stale_days,
        tables,
        total,
        stale_count,
        never_read_count,
    })
}

/// Per-table consumer breakdown: who read it and through what.
fn consumers_query(full_name: &str) -> String {
    let fq = lit(full_name);
    format!(
        "SELECT COALESCE(entity_type, 'EXTERNAL') AS entity_type, \
                COALESCE(created_by, 'unknown') AS principal, \
                COUNT(*) AS events, \
                CAST(datediff(current_timestamp(), MAX(event_time)) AS INT) AS days_ago \
         FROM system.access.table_lineage \
         WHERE source_table_full_name = '{fq}' \
           AND event_time >= date_sub(current_timestamp(), {WINDOW_DAYS}) \
         GROUP BY 1, 2 \
         ORDER BY events DESC \
         LIMIT 20"
    )
}

/// Read and write recency plus per-window read counts for one table.
fn table_summary_query(full_name: &str) -> String {
    let fq = lit(full_name);
    format!(
        "SELECT CAST(datediff(current_timestamp(), MAX(CASE WHEN source_table_full_name = '{fq}' \
                    THEN event_time END)) AS INT) AS days_since_read, \
                CAST(datediff(current_timestamp(), MAX(CASE WHEN target_table_full_name = '{fq}' \
                    THEN event_time END)) AS INT) AS days_since_write, \
                SUM(CASE WHEN source_table_full_name = '{fq}' \
                    AND event_time >= date_sub(current_timestamp(), 30) THEN 1 ELSE 0 END) AS reads_30d, \
                SUM(CASE WHEN source_table_full_name = '{fq}' \
                    AND event_time >= date_sub(current_timestamp(), 90) THEN 1 ELSE 0 END) AS reads_90d, \
                SUM(CASE WHEN source_table_full_name = '{fq}' THEN 1 ELSE 0 END) AS reads_window, \
                COUNT(DISTINCT CASE WHEN source_table_full_name = '{fq}' THEN created_by END) AS readers \
         FROM system.access.table_lineage \
         WHERE (source_table_full_name = '{fq}' OR target_table_full_name = '{fq}') \
           AND event_time >= date_sub(current_timestamp(), {WINDOW_DAYS})"
    )
}

/// Usage of one table as a detail pane: recency, read counts per window,
/// and the principals and entity kinds doing the reading.
pub async fn table_detail(
    cli: &DatabricksCli,
    full_name: &str,
    warehouse_id: &str,
    stale_days: i64,
) -> crate::shape::DetailData {
    use crate::shape::Status;

    let summary_sql = table_summary_query(full_name);
    let consumers_sql = consumers_query(full_name);
    let (summary_res, consumers_res) = tokio::join!(
        run_sql(cli, &summary_sql, warehouse_id),
        run_sql(cli, &consumers_sql, warehouse_id),
    );

    let table = match summary_res {
        Ok(t) => t,
        Err(e) => return usage_error(e),
    };

    let cell = |i: usize| -> Option<i64> {
        table
            .rows
            .first()
            .and_then(|r| r.get(i))
            .filter(|c| *c != "␀")
            .and_then(|c| c.parse::<i64>().ok())
    };
    let days_since_read = cell(0);
    let days_since_write = cell(1);
    let reads_30d = cell(2).unwrap_or(0);
    let reads_90d = cell(3).unwrap_or(0);
    let reads_window = cell(4).unwrap_or(0);
    let readers = cell(5).unwrap_or(0);

    let verdict = match days_since_read {
        None => format!("⚠ no read in {WINDOW_DAYS} days"),
        Some(d) if d >= stale_days => format!("⚠ stale — no read in {d} days"),
        Some(d) => format!("✓ read {d} days ago"),
    };

    let summary = vec![
        ("Object".to_string(), full_name.to_string()),
        ("Verdict".to_string(), verdict),
        (
            "Last read".to_string(),
            days_since_read
                .map(|d| format!("{d}d ago"))
                .unwrap_or_else(|| format!("never in {WINDOW_DAYS}d")),
        ),
        (
            "Last written".to_string(),
            days_since_write
                .map(|d| format!("{d}d ago"))
                .unwrap_or_else(|| format!("never in {WINDOW_DAYS}d")),
        ),
        (
            "Reads".to_string(),
            format!("{reads_30d} in 30d · {reads_90d} in 90d · {reads_window} in {WINDOW_DAYS}d"),
        ),
        ("Distinct readers".to_string(), readers.to_string()),
        (
            "Threshold".to_string(),
            format!("stale after {stale_days} days"),
        ),
    ];

    let mut activity: Vec<(Status, String)> = Vec::new();
    match consumers_res {
        Ok(c) if !c.rows.is_empty() => {
            activity.push((Status::Success, "CONSUMERS · who reads it".to_string()));
            for row in &c.rows {
                let [entity, principal, events, days_ago] = row.as_slice() else {
                    continue;
                };
                // A consumer that itself went quiet is worth seeing.
                let status = match days_ago.parse::<i64>() {
                    Ok(d) if d >= stale_days => Status::Stopped,
                    Ok(_) => Status::Success,
                    Err(_) => Status::Unknown(String::new()),
                };
                activity.push((
                    status,
                    format!("{entity:<16} {principal:<34} {events:>6} reads · {days_ago}d ago"),
                ));
            }
        }
        Ok(_) => activity.push((
            Status::Unknown(String::new()),
            format!("no read events in the last {WINDOW_DAYS} days"),
        )),
        Err(e) => activity.push((Status::Failed, format!("consumers unavailable: {e}"))),
    }

    let raw = activity
        .iter()
        .map(|(_, l)| l.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    crate::shape::DetailData {
        summary,
        activity,
        raw,
    }
}

fn usage_error(e: String) -> crate::shape::DetailData {
    crate::shape::DetailData {
        summary: Vec::new(),
        activity: Vec::new(),
        raw: format!(
            "{e}\n\nusage needs read access to system.access.table_lineage\n\n\
             note: lineage only records access through Unity Catalog-governed \
             compute. Reads of the underlying cloud files, non-UC clusters and \
             hive_metastore tables never appear here, so \"no reads\" is evidence, \
             not proof."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(days_since_read: &str, reads: &str, age: &str) -> Vec<String> {
        vec![
            "main.sales.orders".to_string(),
            "sales".to_string(),
            "MANAGED".to_string(),
            days_since_read.to_string(),
            "3".to_string(),
            reads.to_string(),
            "2".to_string(),
            age.to_string(),
        ]
    }

    /// The whole point of the LEFT JOIN: a table with no lineage rows at
    /// all comes back with a NULL last-read, and that has to classify as
    /// the worst case rather than parsing to zero days and looking fresh.
    #[test]
    fn null_last_read_is_never_read_not_freshly_read() {
        let t = parse_row(&row("␀", "0", "400"), 90).unwrap();
        assert_eq!(t.freshness, Freshness::NeverRead);
        assert_eq!(t.days_since_read, None);
        assert!(t.note().starts_with("no reads in 365d"));
    }

    #[test]
    fn threshold_is_inclusive_at_the_boundary() {
        assert_eq!(
            parse_row(&row("89", "1", "400"), 90).unwrap().freshness,
            Freshness::Active
        );
        assert_eq!(
            parse_row(&row("90", "1", "400"), 90).unwrap().freshness,
            Freshness::Stale
        );
    }

    /// A user-configured threshold has to actually move the boundary,
    /// not just relabel the default one.
    #[test]
    fn custom_threshold_reclassifies() {
        let r = row("100", "1", "400");
        assert_eq!(parse_row(&r, 90).unwrap().freshness, Freshness::Stale);
        assert_eq!(parse_row(&r, 120).unwrap().freshness, Freshness::Active);
    }

    /// A never-read table that was created last week is not evidence of
    /// waste, so its note has to carry the age that explains it.
    #[test]
    fn never_read_note_carries_age_so_new_tables_read_as_new() {
        let t = parse_row(&row("␀", "0", "3"), 90).unwrap();
        assert!(t.note().contains("created 3d ago"), "note: {}", t.note());
    }

    #[test]
    fn flagged_excludes_active_tables() {
        let tables = vec![
            parse_row(&row("5", "10", "400"), 90).unwrap(),
            parse_row(&row("200", "1", "400"), 90).unwrap(),
            parse_row(&row("␀", "0", "400"), 90).unwrap(),
        ];
        let scan = UsageScan {
            scope: "main".to_string(),
            stale_days: 90,
            total: 3,
            stale_count: 1,
            never_read_count: 1,
            tables,
        };
        assert_eq!(scan.flagged().count(), 2);
    }

    /// Reads are attributed via `source_table_full_name`; counting a row
    /// where the table is the *target* would make a pipeline writing into
    /// a table nobody consumes look actively used.
    #[test]
    fn scan_query_reads_come_from_the_source_side() {
        let q = scan_query("main", None);
        assert!(q.contains("SELECT source_table_full_name AS full_name"));
        assert!(q.contains("source_table_catalog = 'main'"));
        // information_schema drives the join, so never-read rows survive.
        assert!(q.contains("LEFT JOIN reads r"));
        assert!(q.contains("main`.information_schema.tables t"));
    }

    #[test]
    fn scan_query_narrows_to_a_schema_when_given_one() {
        let q = scan_query("main", Some("sales"));
        assert!(q.contains("t.table_schema = 'sales'"));
        assert!(!scan_query("main", None).contains("t.table_schema = '"));
    }

    /// Names reach these queries from user input via the catalog tree; a
    /// quote in one must not end the literal.
    #[test]
    fn identifiers_and_literals_are_escaped() {
        let q = scan_query("we'ird", Some("s'chema"));
        assert!(q.contains("source_table_catalog = 'we''ird'"));
        assert!(q.contains("t.table_schema = 's''chema'"));
        assert!(consumers_query("a.b.o'brien").contains("'a.b.o''brien'"));
    }

    #[test]
    fn catalog_identifier_is_backtick_quoted_for_hyphenated_names() {
        assert!(scan_query("my-catalog", None).contains("`my-catalog`.information_schema.tables"));
    }
}
