use crate::cli::DatabricksCli;
use crate::fetchers;
use crate::shape::{DetailData, Shape, Status};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Dark,
    Light,
    CatppuccinMocha,
    CatppuccinLatte,
    GruvboxDark,
    Dracula,
    Nord,
    TokyoNight,
}

impl ThemeMode {
    pub const ALL: &'static [ThemeMode] = &[
        ThemeMode::Dark,
        ThemeMode::Light,
        ThemeMode::CatppuccinMocha,
        ThemeMode::CatppuccinLatte,
        ThemeMode::GruvboxDark,
        ThemeMode::Dracula,
        ThemeMode::Nord,
        ThemeMode::TokyoNight,
    ];

    /// The next theme in the cycle — what `t` steps through.
    pub fn toggled(self) -> Self {
        let idx = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn name(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark (terminal colors)",
            ThemeMode::Light => "Light",
            ThemeMode::CatppuccinMocha => "Catppuccin Mocha",
            ThemeMode::CatppuccinLatte => "Catppuccin Latte",
            ThemeMode::GruvboxDark => "Gruvbox Dark",
            ThemeMode::Dracula => "Dracula",
            ThemeMode::Nord => "Nord",
            ThemeMode::TokyoNight => "Tokyo Night",
        }
    }

    /// Stable id, same kebab-case form the --theme flag accepts.
    pub fn id(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
            ThemeMode::CatppuccinMocha => "catppuccin-mocha",
            ThemeMode::CatppuccinLatte => "catppuccin-latte",
            ThemeMode::GruvboxDark => "gruvbox",
            ThemeMode::Dracula => "dracula",
            ThemeMode::Nord => "nord",
            ThemeMode::TokyoNight => "tokyo-night",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.id() == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    Clusters,
    Jobs,
    Pipelines,
    Warehouses,
    Dashboards,
    Catalog,
    Secrets,
}

impl Panel {
    pub const ALL: &'static [Panel] = &[
        Panel::Clusters,
        Panel::Jobs,
        Panel::Pipelines,
        Panel::Warehouses,
        Panel::Dashboards,
        Panel::Catalog,
        Panel::Secrets,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Panel::Clusters => "Compute",
            Panel::Jobs => "Lakeflow Jobs",
            Panel::Pipelines => "Lakeflow Pipelines",
            Panel::Warehouses => "SQL Warehouses",
            Panel::Dashboards => "AI/BI Dashboards",
            Panel::Catalog => "Unity Catalog",
            Panel::Secrets => "Secret Scopes",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Panel::Clusters => "⌬",
            Panel::Jobs => "⟳",
            Panel::Pipelines => "⋙",
            Panel::Warehouses => "⌁",
            Panel::Dashboards => "▦",
            Panel::Catalog => "⧉",
            Panel::Secrets => "◈",
        }
    }

    /// Stable id used in the config file.
    pub fn id(&self) -> &'static str {
        match self {
            Panel::Clusters => "compute",
            Panel::Jobs => "jobs",
            Panel::Pipelines => "pipelines",
            Panel::Warehouses => "warehouses",
            Panel::Dashboards => "dashboards",
            Panel::Catalog => "catalog",
            Panel::Secrets => "secrets",
        }
    }

    /// The databricks CLI command group whose `get <id>` returns item details.
    pub fn cli_group(&self) -> &'static str {
        match self {
            Panel::Clusters => "clusters",
            Panel::Jobs => "jobs",
            Panel::Pipelines => "pipelines",
            Panel::Warehouses => "warehouses",
            Panel::Dashboards => "lakeview",
            Panel::Catalog => "tables",
            Panel::Secrets => "secrets",
        }
    }
}

pub struct Detail {
    pub panel: Panel,
    pub name: String,
    pub id: String,
    /// Item kind for Unity Catalog leaves (TABLE / VIEW / VOLUME).
    pub kind: Option<String>,
    /// Heading of the activity section ("Recent activity", "Access", ...).
    pub section: &'static str,
    /// None while the fetch is in flight.
    pub data: Option<DetailData>,
    /// Toggles between the formatted summary and the raw JSON.
    pub show_raw: bool,
    pub scroll: u16,
}

/// Full-screen sample-data view for a Unity Catalog table or view.
pub struct Preview {
    pub name: String,
    /// Display name and id of the warehouse running the query.
    pub warehouse: String,
    pub warehouse_id: String,
    /// None while the query runs; then rows or an error.
    pub data: Option<Result<crate::shape::TableData, String>>,
    /// Top visible row in the grid; the inspected row in record view.
    pub scroll: usize,
    /// First visible column, as an index into the filtered column list.
    pub col: usize,
    /// Case-insensitive substring filter over column names — the way
    /// through a 500-column table.
    pub filter: String,
    pub filter_entry: bool,
    /// Transposed view: one row, fields stacked vertically.
    pub record: bool,
    /// Field scroll within the record view.
    pub rscroll: u16,
}

impl Preview {
    /// Indices of columns whose name matches the filter (all when empty).
    pub fn visible_cols(&self) -> Vec<usize> {
        let Some(Ok(t)) = &self.data else {
            return Vec::new();
        };
        let q = self.filter.to_lowercase();
        (0..t.headers.len())
            .filter(|&i| q.is_empty() || t.headers[i].to_lowercase().contains(&q))
            .collect()
    }
}

/// What a confirmed warehouse choice should run.
enum PickTarget {
    Preview(String),
    Cost,
    Lineage(String),
    Sql(String),
}

/// Free-form SQL prompt with results, backed by the preview machinery.
pub struct SqlConsole {
    pub input: String,
    /// Caret position in `input`, counted in characters.
    pub cursor: usize,
    /// Display name of the warehouse the last query ran on.
    pub warehouse: String,
    pub running: bool,
    pub data: Option<Result<crate::shape::TableData, String>>,
    /// The statement that produced `data`.
    pub last_sql: String,
    pub scroll: usize,
    /// First visible result column (shift+←/→ pages wide results).
    pub col: usize,
}

/// Tab-completion popup over the SQL prompt, backed by lazily-cached
/// Unity Catalog names.
pub struct SqlComplete {
    /// Candidates for the segment being completed; empty while loading.
    pub items: Vec<String>,
    /// The candidate currently inserted into the prompt.
    pub index: usize,
    /// Char offset in the input where the completed segment starts —
    /// the popup anchors under it.
    pub seg_start: usize,
    /// The typed prefix, restored on esc.
    prefix: String,
    /// Dotted context before the segment ("" for a bare word).
    context: String,
    /// True while names are being fetched from the workspace.
    pub loading: bool,
}

/// Completed alongside catalog names when a bare word is typed; also
/// the word list for prompt syntax highlighting.
pub(crate) const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "JOIN",
    "LEFT JOIN",
    "INNER JOIN",
    "FULL OUTER JOIN",
    "CROSS JOIN",
    "ON",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IN",
    "IS",
    "NULL",
    "DISTINCT",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "UNION",
    "UNION ALL",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "SHOW",
    "DESCRIBE",
    "EXPLAIN",
    "WITH",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "BETWEEN",
    "LIKE",
    "CAST",
    "OVER",
    "PARTITION BY",
];

/// The dotted identifier ending at the caret: (char offset where its
/// last segment starts, context path before the last dot, typed prefix).
fn token_at_cursor(input: &str, cursor: usize) -> (usize, String, String) {
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());
    let mut start = cursor;
    while start > 0 && (chars[start - 1].is_alphanumeric() || matches!(chars[start - 1], '_' | '.'))
    {
        start -= 1;
    }
    let token: String = chars[start..cursor].iter().collect();
    match token.rfind('.') {
        Some(dot) => {
            let context = token[..dot].to_string();
            let prefix = token[dot + 1..].to_string();
            (start + context.chars().count() + 1, context, prefix)
        }
        None => (start, String::new(), token),
    }
}

/// The first table referenced by a FROM clause, when fully qualified —
/// its columns join the candidates for bare words.
fn from_table(input: &str) -> Option<String> {
    let pos = input.to_lowercase().find("from ")?;
    // get(): lowercasing can shift byte offsets in non-ASCII input.
    let rest = input.get(pos + 5..)?.trim_start();
    let table: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
        .collect();
    (table.matches('.').count() == 2 && !table.ends_with('.')).then_some(table)
}

/// Where console history lives; one statement per line, oldest first.
fn history_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::PathBuf::from(home)
            .join(".config")
            .join("databricks-tui")
            .join("history"),
    )
}

fn load_history() -> Vec<String> {
    history_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| {
            s.lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn save_history(history: &[String]) {
    let Some(path) = history_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        crate::config::restrict(dir, 0o700);
    }
    // Keep the tail; nobody scrolls back 200 queries.
    let tail: Vec<&str> = history
        .iter()
        .rev()
        .take(200)
        .rev()
        .map(String::as_str)
        .collect();
    // Queries can hold sensitive literals — owner-only, like shell history.
    let _ = std::fs::write(&path, tail.join("\n") + "\n");
    crate::config::restrict(&path, 0o600);
}

/// True when every char of `needle` appears in `haystack` in order.
fn subsequence(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|n| chars.any(|h| h == n))
}

/// Byte offset of the `cursor`th character in `input`.
fn byte_at(input: &str, cursor: usize) -> usize {
    input
        .char_indices()
        .nth(cursor)
        .map(|(i, _)| i)
        .unwrap_or(input.len())
}

/// Overlay for choosing which SQL warehouse runs a query.
pub struct WhPicker {
    pub index: usize,
    target: PickTarget,
}

/// Full-screen DBU usage view backed by system.billing.usage.
pub struct CostView {
    pub warehouse: String,
    pub data: Option<Result<fetchers::cost::CostData, String>>,
}

/// Drill-down into a single job run or pipeline update, layered over
/// the owning detail view.
pub struct RunView {
    /// Panel::Jobs (runs) or Panel::Pipelines (updates).
    pub panel: Panel,
    pub owner_name: String,
    /// Job id or pipeline id the runs belong to.
    owner_id: String,
    /// Recent runs newest-first: (run_id, status, age).
    pub runs: Vec<(String, Status, String)>,
    /// Which of `runs` is shown.
    pub idx: usize,
    pub data: Option<DetailData>,
    pub show_raw: bool,
    pub scroll: u16,
    /// True while the shown run is still executing — drives auto-refresh.
    pub live: bool,
    /// Full per-task output/logs, fetched on demand via `o`.
    pub output: Option<String>,
    pub show_output: bool,
    /// Gantt view of per-task execution windows; sticky across h/l so
    /// runs can be compared.
    pub show_timeline: bool,
    /// Dependency-tree view of the run's tasks; mutually exclusive with
    /// the timeline, sticky across h/l like it.
    pub show_dag: bool,
    /// History grid: tasks × recent runs, with duration trends. Fetched
    /// once per run view (the grid is per-job, not per-run).
    pub show_grid: bool,
    pub grid: Option<Result<fetchers::runs::GridData, String>>,
    fetched_at: Instant,
}

/// (recent runs, detail of the newest, still-executing flag)
type RunOpened = (Vec<(String, Status, String)>, DetailData, bool);

enum RunUpdate {
    Opened(Result<RunOpened, String>),
    Detail(DetailData, bool),
    /// Full task output plus whether the run is still executing.
    Output(String, bool),
}

/// One unhealthy resource, pointing back at its pane and item.
pub struct Problem {
    /// Index into `Panel::ALL`; None when the problem is a whole
    /// workspace being unreachable during a cross-workspace scan.
    pub panel: Option<usize>,
    pub name: String,
    pub status: Status,
    pub note: String,
    /// Some(profile) when the problem lives in another workspace.
    pub profile: Option<String>,
}

/// Overlay collecting everything currently failing: the loaded panes
/// immediately, plus every other configured workspace as the scan of
/// their profiles comes back.
pub struct Problems {
    pub items: Vec<Problem>,
    pub index: usize,
    /// True while other profiles are still being scanned.
    pub scanning: bool,
}

/// Overlay listing jobs by their next scheduled execution, soonest first.
pub struct Upcoming {
    pub items: Vec<fetchers::upcoming::UpcomingJob>,
    pub index: usize,
    pub loading: bool,
}

/// A pending destructive/mutating action awaiting a y/n keystroke.
pub struct Confirm {
    pub message: String,
    args: Vec<String>,
}

enum Update {
    Panel(usize, Result<Shape, String>),
    Badge(Option<Shape>),
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct App {
    pub focus: Panel,
    pub theme: ThemeMode,
    pub zoomed: bool,
    pub shapes: Vec<Option<Shape>>,
    pub user_badge: Option<Shape>,
    pub error: Option<String>,
    pub refresh_interval: Duration,
    last_refresh: Instant,
    pub loading: bool,
    pub detail: Option<Detail>,
    pub confirm: Option<Confirm>,
    pub flash: Option<(String, Instant)>,
    pub selected: [usize; 7],
    pub host: Option<String>,
    /// Available profiles from ~/.databrickscfg and the active one.
    pub profiles: Vec<String>,
    pub profile: Option<String>,
    /// When Some, the workspace picker overlay is open at this index.
    pub picker: Option<usize>,
    /// When Some, the problems overlay is open.
    pub problems: Option<Problems>,
    pub upcoming: Option<Upcoming>,
    /// Current position in the Unity Catalog tree: [], [catalog] or [catalog, schema].
    pub uc_path: Vec<String>,
    uc_rx: Option<oneshot::Receiver<Result<Shape, String>>>,
    pub preview: Option<Preview>,
    preview_rx: Option<oneshot::Receiver<Result<crate::shape::TableData, String>>>,
    pub wh_picker: Option<WhPicker>,
    /// Session-remembered (id, name) of the warehouse used for previews.
    pub preview_warehouse: Option<(String, String)>,
    pub cost: Option<CostView>,
    #[allow(clippy::type_complexity)]
    cost_rx: Option<oneshot::Receiver<(Result<fetchers::cost::CostData, String>, Option<String>)>>,
    /// Numeric id of the current workspace, resolved lazily for cost
    /// scoping and cached for the session.
    workspace_id: Option<String>,
    pub sql: Option<SqlConsole>,
    sql_rx: Option<oneshot::Receiver<Result<crate::shape::TableData, String>>>,
    /// Past console statements, oldest first; persisted across sessions.
    sql_history: Vec<String>,
    /// Position while cycling history with ↑/↓; None = editing a new line.
    hist_idx: Option<usize>,
    /// The unfinished statement stashed when history browsing starts.
    hist_draft: String,
    /// Ctrl+R incremental search: (query, nth-newest match shown).
    pub hist_search: Option<(String, usize)>,
    pub run_view: Option<RunView>,
    run_rx: Option<oneshot::Receiver<RunUpdate>>,
    grid_rx: Option<oneshot::Receiver<Result<fetchers::runs::GridData, String>>>,
    #[allow(clippy::type_complexity)]
    upcoming_rx: Option<oneshot::Receiver<Result<Vec<fetchers::upcoming::UpcomingJob>, String>>>,
    problems_rx: Option<oneshot::Receiver<Vec<fetchers::problems::RemoteProblem>>>,
    pending: Option<mpsc::UnboundedReceiver<Update>>,
    detail_rx: Option<oneshot::Receiver<DetailData>>,
    action_rx: Option<oneshot::Receiver<Result<String, String>>>,
    host_rx: Option<oneshot::Receiver<Option<String>>>,
    in_flight: usize,
    spinner_frame: usize,
    /// Splash screen deadline; None once dismissed.
    pub splash_until: Option<Instant>,
    /// When each pane last received fresh data — drives the title flash.
    pub updated_at: [Option<Instant>; 7],
    /// Per-pane search filter; empty string means no filter.
    pub filters: [String; 7],
    /// True while the user is typing a filter for the focused pane.
    pub filter_entry: bool,
    /// Persisted preferences (theme, warehouse per profile).
    pub config: crate::config::Config,
    /// Failed item names per pane at the last refresh — None until the
    /// pane has loaded once, so the first load never alerts.
    failed_seen: [Option<std::collections::HashSet<String>>; 7],
    /// Ctrl+P fuzzy jump overlay.
    pub jump: Option<Jump>,
    /// Canonical pane indices in display order.
    pub pane_order: Vec<usize>,
    /// Hidden flag per canonical pane index.
    pub hidden: [bool; 7],
    /// When Some, the pane-arrangement overlay is open at this position.
    pub pane_cfg: Option<usize>,
    /// True while the `?` help overlay is open.
    pub help: bool,
    /// Scroll offset of the help overlay.
    pub help_scroll: u16,
    /// Statement id of the in-flight console query, for cancellation.
    sql_stmt: Option<std::sync::Arc<std::sync::Mutex<Option<String>>>>,
    /// Tab-completion popup state for the SQL prompt.
    pub sql_complete: Option<SqlComplete>,
    /// Unity Catalog names for completion, keyed by dotted path ("" →
    /// catalogs, "cat" → schemas, …). Filled lazily, kept per workspace.
    uc_names: std::collections::HashMap<String, Vec<String>>,
    #[allow(clippy::type_complexity)]
    uc_names_rx: Option<oneshot::Receiver<(String, Result<Vec<String>, String>)>>,
    /// Drilled-into secret scope; None = the scopes listing.
    pub secret_scope: Option<String>,
    secrets_rx: Option<oneshot::Receiver<Result<Shape, String>>>,
    /// Create-scope / add-secret input form.
    pub secret_form: Option<SecretForm>,
}

/// Ctrl+P overlay: fuzzy-search everything loaded, Enter jumps to it.
pub struct Jump {
    pub query: String,
    pub index: usize,
}

/// Two-step input: a scope name, or a key then a (masked) value.
pub struct SecretForm {
    /// Scope the secret goes into; None = creating a new scope.
    pub scope: Option<String>,
    pub key: String,
    pub value: String,
    /// 0 = typing the name/key, 1 = typing the value.
    pub stage: u8,
}

impl App {
    pub fn new(refresh_secs: u64, theme: ThemeMode) -> Self {
        let mut app = Self {
            focus: Panel::Clusters,
            theme,
            zoomed: false,
            shapes: vec![None; 7],
            user_badge: None,
            error: None,
            refresh_interval: Duration::from_secs(refresh_secs),
            last_refresh: Instant::now()
                .checked_sub(Duration::from_secs(refresh_secs + 1))
                .unwrap_or(Instant::now()),
            loading: false,
            detail: None,
            confirm: None,
            flash: None,
            selected: [0; 7],
            host: None,
            profiles: Vec::new(),
            profile: None,
            picker: None,
            problems: None,
            upcoming: None,
            uc_path: Vec::new(),
            uc_rx: None,
            preview: None,
            preview_rx: None,
            wh_picker: None,
            preview_warehouse: None,
            cost: None,
            cost_rx: None,
            workspace_id: None,
            sql: None,
            sql_rx: None,
            sql_history: load_history(),
            hist_idx: None,
            hist_draft: String::new(),
            hist_search: None,
            run_view: None,
            run_rx: None,
            grid_rx: None,
            upcoming_rx: None,
            problems_rx: None,
            pending: None,
            detail_rx: None,
            action_rx: None,
            host_rx: None,
            in_flight: 0,
            spinner_frame: 0,
            splash_until: Some(Instant::now() + Duration::from_millis(1600)),
            updated_at: [None; 7],
            filters: Default::default(),
            filter_entry: false,
            config: crate::config::Config::load(),
            failed_seen: Default::default(),
            jump: None,
            pane_order: (0..7).collect(),
            hidden: [false; 7],
            pane_cfg: None,
            help: false,
            help_scroll: 0,
            sql_stmt: None,
            sql_complete: None,
            uc_names: Default::default(),
            uc_names_rx: None,
            secret_scope: None,
            secrets_rx: None,
            secret_form: None,
        };
        app.load_pane_prefs();
        app
    }

    /// Applies pane order/visibility from the config file.
    fn load_pane_prefs(&mut self) {
        let idx_of = |id: &str| Panel::ALL.iter().position(|p| p.id() == id);
        let mut order: Vec<usize> = self
            .config
            .pane_order
            .iter()
            .filter_map(|id| idx_of(id))
            .collect();
        for i in 0..7 {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        self.pane_order = order;
        for id in &self.config.hidden_panes {
            if let Some(i) = idx_of(id) {
                self.hidden[i] = true;
            }
        }
        self.ensure_focus_visible();
    }

    fn persist_panes(&mut self) {
        self.config.pane_order = self
            .pane_order
            .iter()
            .map(|&i| Panel::ALL[i].id().to_string())
            .collect();
        self.config.hidden_panes = (0..7)
            .filter(|&i| self.hidden[i])
            .map(|i| Panel::ALL[i].id().to_string())
            .collect();
        self.config.save();
    }

    /// Canonical pane indices currently shown, in display order.
    pub fn visible_panes(&self) -> Vec<usize> {
        self.pane_order
            .iter()
            .copied()
            .filter(|&i| !self.hidden[i])
            .collect()
    }

    /// Moves focus off a hidden pane onto the first visible one.
    fn ensure_focus_visible(&mut self) {
        let visible = self.visible_panes();
        let focus_idx = Panel::ALL
            .iter()
            .position(|p| p == &self.focus)
            .unwrap_or(0);
        if !visible.contains(&focus_idx) {
            if let Some(&first) = visible.first() {
                self.focus = Panel::ALL[first];
            }
        }
    }

    /// Unhides a pane (used when a jump targets it).
    fn reveal_pane(&mut self, idx: usize) {
        if self.hidden[idx] {
            self.hidden[idx] = false;
            self.persist_panes();
        }
    }

    pub fn open_pane_cfg(&mut self) {
        self.pane_cfg = Some(0);
    }

    pub fn pane_cfg_next(&mut self) {
        if let Some(i) = self.pane_cfg {
            self.pane_cfg = Some((i + 1).min(6));
        }
    }

    pub fn pane_cfg_prev(&mut self) {
        if let Some(i) = self.pane_cfg {
            self.pane_cfg = Some(i.saturating_sub(1));
        }
    }

    /// Space in the overlay: toggles visibility of the selected pane
    /// (refusing to hide the last visible one).
    pub fn pane_cfg_toggle(&mut self) {
        let Some(pos) = self.pane_cfg else {
            return;
        };
        let idx = self.pane_order[pos];
        if !self.hidden[idx] && self.visible_panes().len() == 1 {
            self.flash = Some((
                "✗ at least one pane has to stay visible".to_string(),
                Instant::now(),
            ));
            return;
        }
        self.hidden[idx] = !self.hidden[idx];
        self.ensure_focus_visible();
        self.persist_panes();
    }

    /// J/K in the overlay: moves the selected pane down/up in the order.
    pub fn pane_cfg_move(&mut self, delta: i32) {
        let Some(pos) = self.pane_cfg else {
            return;
        };
        let new = if delta < 0 {
            pos.saturating_sub(1)
        } else {
            (pos + 1).min(6)
        };
        if new != pos {
            self.pane_order.swap(pos, new);
            self.pane_cfg = Some(new);
            self.persist_panes();
        }
    }

    /// Flashes (and rings the bell) when a resource fails between one
    /// refresh and the next.
    fn alert_new_failures(&mut self, idx: usize) {
        // Catalog "error rows" are listing problems, not runtime failures.
        if idx >= 5 {
            return;
        }
        let Some(Shape::List(items)) = &self.shapes[idx] else {
            return;
        };
        let failed: std::collections::HashSet<String> = items
            .iter()
            .filter(|it| {
                matches!(it.status, Status::Failed)
                    || it
                        .history
                        .last()
                        .is_some_and(|s| matches!(s, Status::Failed))
            })
            .map(|it| it.name.clone())
            .collect();
        if let Some(prev) = &self.failed_seen[idx] {
            let mut newly: Vec<&String> = failed.difference(prev).collect();
            if !newly.is_empty() {
                newly.sort();
                let extra = if newly.len() > 1 {
                    format!(" (+{} more)", newly.len() - 1)
                } else {
                    String::new()
                };
                self.flash = Some((
                    format!("✗ {}{extra} just failed — ! to inspect", newly[0]),
                    Instant::now(),
                ));
                // Bell so a backgrounded terminal (or tmux) flags it too.
                print!("\x07");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        }
        self.failed_seen[idx] = Some(failed);
    }

    /// Remembers the current theme across sessions.
    pub fn persist_theme(&mut self) {
        self.config.theme = Some(self.theme.id().to_string());
        self.config.save();
    }

    /// Restores the remembered warehouse for the active profile.
    pub fn restore_warehouse_pref(&mut self) {
        let profile = self.profile.as_deref().unwrap_or("DEFAULT");
        self.preview_warehouse = self.config.warehouses.get(profile).cloned();
    }

    pub fn splash_active(&self) -> bool {
        self.splash_until
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }

    pub fn dismiss_splash(&mut self) {
        self.splash_until = None;
    }

    /// True while any pane's data just landed — keeps the flash decaying.
    pub fn any_fresh(&self) -> bool {
        self.updated_at
            .iter()
            .flatten()
            .any(|t| t.elapsed() < Duration::from_millis(1200))
    }

    pub fn open_picker(&mut self) {
        if self.profiles.is_empty() {
            return;
        }
        let current = self
            .profile
            .as_deref()
            .and_then(|p| self.profiles.iter().position(|n| n == p))
            .unwrap_or(0);
        self.picker = Some(current);
    }

    pub fn picker_next(&mut self) {
        if let Some(i) = self.picker {
            self.picker = Some((i + 1).min(self.profiles.len().saturating_sub(1)));
        }
    }

    pub fn picker_prev(&mut self) {
        if let Some(i) = self.picker {
            self.picker = Some(i.saturating_sub(1));
        }
    }

    /// Confirms the picker selection; returns the new CLI handle to use.
    pub fn picker_select(&mut self) -> Option<Arc<DatabricksCli>> {
        let idx = self.picker.take()?;
        let name = self.profiles.get(idx)?.clone();
        Some(self.switch_profile(name))
    }

    /// Switches to `name`, dropping all workspace-specific state, and
    /// returns the CLI handle for the new workspace.
    fn switch_profile(&mut self, name: String) -> Arc<DatabricksCli> {
        let profile_arg = if name == "DEFAULT" {
            None
        } else {
            Some(name.clone())
        };
        self.profile = Some(name);

        // Drop all workspace-specific state; panes go back to loading.
        self.shapes = vec![None; 7];
        self.user_badge = None;
        self.host = None;
        self.selected = [0; 7];
        self.detail = None;
        self.detail_rx = None;
        self.confirm = None;
        self.problems = None;
        self.problems_rx = None;
        self.uc_path.clear();
        self.uc_rx = None;
        self.secret_scope = None;
        self.secrets_rx = None;
        self.secret_form = None;
        self.preview = None;
        self.preview_rx = None;
        self.wh_picker = None;
        self.preview_warehouse = None;
        self.cost = None;
        self.cost_rx = None;
        self.workspace_id = None;
        self.sql = None;
        self.sql_rx = None;
        self.run_view = None;
        self.run_rx = None;
        self.grid_rx = None;
        self.pending = None;
        self.in_flight = 0;
        self.loading = false;
        self.zoomed = false;
        self.filters = Default::default();
        self.filter_entry = false;
        self.failed_seen = Default::default();
        self.jump = None;
        self.sql_stmt = None;
        self.sql_complete = None;
        self.uc_names.clear();
        self.uc_names_rx = None;
        self.restore_warehouse_pref();

        Arc::new(DatabricksCli::new(profile_arg))
    }

    pub fn open_jump(&mut self) {
        self.jump = Some(Jump {
            query: String::new(),
            index: 0,
        });
    }

    /// Everything loaded that matches the jump query, best first:
    /// (panel index, item name, kind/status label). Substring matches
    /// rank above in-order subsequence matches.
    pub fn jump_matches(&self) -> Vec<(usize, String, String)> {
        let Some(jump) = &self.jump else {
            return Vec::new();
        };
        let q = jump.query.to_lowercase();
        let mut scored: Vec<(u8, usize, String, String)> = Vec::new();
        for (i, shape) in self.shapes.iter().enumerate() {
            let Some(Shape::List(items)) = shape else {
                continue;
            };
            for it in items {
                let name = it.name.to_lowercase();
                let rank = if q.is_empty() || name.contains(&q) {
                    0
                } else if subsequence(&name, &q) {
                    1
                } else {
                    continue;
                };
                scored.push((rank, i, it.name.clone(), it.status.label().to_string()));
            }
        }
        scored.sort_by(|a, b| (a.0, a.2.len(), &a.2).cmp(&(b.0, b.2.len(), &b.2)));
        scored
            .into_iter()
            .take(12)
            .map(|(_, i, name, label)| (i, name, label))
            .collect()
    }

    pub fn jump_push(&mut self, c: char) {
        if let Some(j) = &mut self.jump {
            j.query.push(c);
            j.index = 0;
        }
    }

    pub fn jump_pop(&mut self) {
        if let Some(j) = &mut self.jump {
            j.query.pop();
            j.index = 0;
        }
    }

    pub fn jump_next(&mut self) {
        let len = self.jump_matches().len();
        if let Some(j) = &mut self.jump {
            j.index = (j.index + 1).min(len.saturating_sub(1));
        }
    }

    pub fn jump_prev(&mut self) {
        if let Some(j) = &mut self.jump {
            j.index = j.index.saturating_sub(1);
        }
    }

    /// Jumps focus and selection to the highlighted match.
    pub fn jump_go(&mut self) {
        let matches = self.jump_matches();
        let Some(jump) = self.jump.take() else {
            return;
        };
        let Some((panel_idx, name, _)) = matches.get(jump.index) else {
            return;
        };
        self.reveal_pane(*panel_idx);
        self.focus = Panel::ALL[*panel_idx];
        self.filters[*panel_idx].clear();
        if let Some(Shape::List(items)) = &self.shapes[*panel_idx] {
            if let Some(pos) = items.iter().position(|i| &i.name == name) {
                self.selected[*panel_idx] = pos;
            }
        }
    }

    /// Collects everything unhealthy across the loaded panes — items
    /// whose status is failed, or whose most recent run failed — then
    /// scans every other configured workspace in the background.
    pub fn open_problems(&mut self) {
        let mut items = Vec::new();
        for (i, shape) in self.shapes.iter().enumerate() {
            let Some(Shape::List(list)) = shape else {
                continue;
            };
            for it in list {
                let failed_now = matches!(it.status, Status::Failed);
                let failed_last = it
                    .history
                    .last()
                    .is_some_and(|s| matches!(s, Status::Failed));
                if failed_now || failed_last {
                    let note = if failed_now {
                        it.detail.clone().unwrap_or_default()
                    } else {
                        "latest run failed".to_string()
                    };
                    items.push(Problem {
                        panel: Some(i),
                        name: it.name.clone(),
                        status: it.status.clone(),
                        note,
                        profile: None,
                    });
                }
            }
        }
        let current = self
            .profile
            .clone()
            .unwrap_or_else(|| "DEFAULT".to_string());
        let others: Vec<String> = self
            .profiles
            .iter()
            .filter(|n| **n != current)
            .cloned()
            .collect();
        let scanning = !others.is_empty();
        if scanning {
            let (tx, rx) = oneshot::channel();
            self.problems_rx = Some(rx);
            tokio::spawn(async move {
                let _ = tx.send(fetchers::problems::fetch(others, current).await);
            });
        }
        self.problems = Some(Problems {
            items,
            index: 0,
            scanning,
        });
    }

    pub fn close_problems(&mut self) {
        self.problems = None;
        self.problems_rx = None;
    }

    pub fn poll_problems(&mut self) -> bool {
        let Some(rx) = &mut self.problems_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(remote) => {
                self.problems_rx = None;
                let Some(pr) = &mut self.problems else {
                    return false;
                };
                pr.scanning = false;
                pr.items.extend(remote.into_iter().map(|r| Problem {
                    panel: r.panel,
                    name: r.name,
                    status: r.status,
                    note: r.note,
                    profile: Some(r.profile),
                }));
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.problems_rx = None;
                if let Some(pr) = &mut self.problems {
                    pr.scanning = false;
                }
                true
            }
        }
    }

    pub fn problems_next(&mut self) {
        if let Some(pr) = &mut self.problems {
            pr.index = (pr.index + 1).min(pr.items.len().saturating_sub(1));
        }
    }

    pub fn problems_prev(&mut self) {
        if let Some(pr) = &mut self.problems {
            pr.index = pr.index.saturating_sub(1);
        }
    }

    /// Jumps focus and selection to the highlighted problem's pane item.
    /// A problem in another workspace switches to that workspace instead;
    /// the returned CLI handle must then replace the current one.
    pub fn problems_jump(&mut self) -> Option<Arc<DatabricksCli>> {
        let Some(pr) = self.problems.take() else {
            self.problems_rx = None;
            return None;
        };
        self.problems_rx = None;
        let problem = pr.items.get(pr.index)?;
        if let Some(profile) = &problem.profile {
            let target = match problem.panel {
                Some(i) => format!("{} is in {}", problem.name, Panel::ALL[i].title()),
                None => "check its auth".to_string(),
            };
            self.flash = Some((
                format!("⌂ switched to {profile} — {target}"),
                Instant::now(),
            ));
            let profile = profile.clone();
            return Some(self.switch_profile(profile));
        }
        let panel = problem.panel?;
        self.reveal_pane(panel);
        self.focus = Panel::ALL[panel];
        // The pane's filter could hide the item we're jumping to.
        self.filters[panel].clear();
        if let Some(Shape::List(list)) = &self.shapes[panel] {
            if let Some(pos) = list.iter().position(|i| i.name == problem.name) {
                self.selected[panel] = pos;
            }
        }
        None
    }

    /// `u`: what runs next — every job with a schedule or trigger,
    /// soonest first, fetched fresh so the countdowns are current.
    pub fn open_upcoming(&mut self, cli: &Arc<DatabricksCli>) {
        self.upcoming = Some(Upcoming {
            items: Vec::new(),
            index: 0,
            loading: true,
        });
        let (tx, rx) = oneshot::channel();
        self.upcoming_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let _ = tx.send(fetchers::upcoming::fetch(&cli).await);
        });
    }

    pub fn close_upcoming(&mut self) {
        self.upcoming = None;
        self.upcoming_rx = None;
    }

    pub fn poll_upcoming(&mut self) -> bool {
        let Some(rx) = &mut self.upcoming_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.upcoming_rx = None;
                match result {
                    Ok(items) => {
                        if let Some(u) = &mut self.upcoming {
                            u.items = items;
                            u.loading = false;
                        }
                    }
                    Err(e) => {
                        self.upcoming = None;
                        let first = e.lines().next().unwrap_or("failed").to_string();
                        self.flash = Some((format!("✗ upcoming: {first}"), Instant::now()));
                    }
                }
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.upcoming_rx = None;
                true
            }
        }
    }

    pub fn upcoming_next(&mut self) {
        if let Some(u) = &mut self.upcoming {
            u.index = (u.index + 1).min(u.items.len().saturating_sub(1));
        }
    }

    pub fn upcoming_prev(&mut self) {
        if let Some(u) = &mut self.upcoming {
            u.index = u.index.saturating_sub(1);
        }
    }

    /// Jumps focus and selection to the highlighted job in the Jobs pane.
    pub fn upcoming_jump(&mut self) {
        let Some(u) = self.upcoming.take() else {
            return;
        };
        self.upcoming_rx = None;
        let Some(item) = u.items.get(u.index) else {
            return;
        };
        let Some(idx) = Panel::ALL.iter().position(|p| *p == Panel::Jobs) else {
            return;
        };
        self.reveal_pane(idx);
        self.focus = Panel::Jobs;
        self.filters[idx].clear();
        if let Some(Shape::List(list)) = &self.shapes[idx] {
            if let Some(pos) = list.iter().position(|i| i.name == item.name) {
                self.selected[idx] = pos;
            }
        }
    }

    /// Resolves the workspace host in the background — `auth describe` can
    /// take seconds when it refreshes tokens, so it must not block the loop.
    pub fn fetch_host(&mut self, cli: &Arc<DatabricksCli>) {
        let (tx, rx) = oneshot::channel();
        self.host_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let host = cli.run(&["auth", "describe"]).await.ok().and_then(|json| {
                json["details"]["host"]
                    .as_str()
                    .or_else(|| json["host"].as_str())
                    .map(str::to_string)
            });
            let _ = tx.send(host);
        });
    }

    pub fn poll_host(&mut self) {
        if let Some(rx) = &mut self.host_rx {
            match rx.try_recv() {
                Ok(host) => {
                    self.host = host;
                    self.host_rx = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.host_rx = None;
                }
            }
        }
    }

    fn focus_index(&self) -> usize {
        Panel::ALL
            .iter()
            .position(|p| p == &self.focus)
            .unwrap_or(0)
    }

    fn list_len(&self, idx: usize) -> usize {
        match &self.shapes[idx] {
            Some(Shape::List(items)) => items
                .iter()
                .filter(|it| crate::shape::item_matches(it, &self.filters[idx]))
                .count(),
            _ => 0,
        }
    }

    /// Selection index for a panel, clamped to the current list length.
    pub fn selection(&self, idx: usize) -> usize {
        self.selected[idx].min(self.list_len(idx).saturating_sub(1))
    }

    pub fn select_next(&mut self) {
        let idx = self.focus_index();
        let len = self.list_len(idx);
        if len > 0 {
            self.selected[idx] = (self.selection(idx) + 1).min(len - 1);
        }
    }

    pub fn select_prev(&mut self) {
        let idx = self.focus_index();
        self.selected[idx] = self.selection(idx).saturating_sub(1);
    }

    /// The currently highlighted item in the focused panel, respecting
    /// the pane's filter — the nth *visible* item, like the UI shows.
    fn selected_item(&self) -> Option<&crate::shape::ListItem> {
        let idx = self.focus_index();
        match &self.shapes[idx] {
            Some(Shape::List(items)) => items
                .iter()
                .filter(|it| crate::shape::item_matches(it, &self.filters[idx]))
                .nth(self.selection(idx)),
            _ => None,
        }
    }

    /// Opens filter entry for the focused pane, starting from scratch.
    pub fn filter_start(&mut self) {
        let idx = self.focus_index();
        self.filters[idx].clear();
        self.selected[idx] = 0;
        self.filter_entry = true;
    }

    pub fn filter_push(&mut self, c: char) {
        let idx = self.focus_index();
        self.filters[idx].push(c);
        self.selected[idx] = 0;
    }

    pub fn filter_pop(&mut self) {
        let idx = self.focus_index();
        self.filters[idx].pop();
        self.selected[idx] = 0;
    }

    /// Keeps the filter applied and returns keys to normal navigation.
    pub fn filter_accept(&mut self) {
        self.filter_entry = false;
    }

    pub fn filter_clear(&mut self) {
        let idx = self.focus_index();
        self.filters[idx].clear();
        self.selected[idx] = 0;
        self.filter_entry = false;
    }

    /// The focused pane's filter, if any.
    pub fn active_filter(&self) -> &str {
        &self.filters[self.focus_index()]
    }

    pub fn open_detail(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(id) = item.id.clone() else {
            return;
        };
        let kind = match &item.status {
            Status::Unknown(k) if !k.is_empty() => Some(k.clone()),
            _ => None,
        };
        let section = match self.focus {
            Panel::Dashboards => "Contents",
            Panel::Catalog => "Columns",
            Panel::Warehouses => "Recent queries",
            _ => "Recent activity",
        };
        self.detail = Some(Detail {
            panel: self.focus,
            name: item.name.clone(),
            id: id.clone(),
            kind,
            section,
            data: None,
            show_raw: false,
            scroll: 0,
        });

        let (tx, rx) = oneshot::channel();
        self.detail_rx = Some(rx);
        let cli = Arc::clone(cli);
        let kind = self.detail.as_ref().unwrap().kind.clone();
        // Files in volumes get a content peek instead of an API `get`.
        if kind.as_deref() == Some("FILE") {
            if let Some(d) = &mut self.detail {
                d.section = "File head";
            }
            tokio::spawn(async move {
                let data = fetchers::catalog::file_peek(&cli, &id).await;
                let _ = tx.send(data);
            });
            return;
        }
        let group = match &kind {
            Some(k) if k == "VOLUME" => "volumes",
            _ => self.focus.cli_group(),
        };
        // Tables get extra facts from DESCRIBE DETAIL when a warehouse
        // is already remembered — free depth, no picker interruption.
        let warehouse = match &kind {
            Some(k) if k == "TABLE" => self.preview_warehouse.clone().map(|(id, _)| id),
            _ => None,
        };
        tokio::spawn(async move {
            let data = fetchers::detail::fetch(&cli, group, &id, warehouse.as_deref()).await;
            let _ = tx.send(data);
        });
    }

    /// Descends one level in the Unity Catalog tree. Returns false when the
    /// selection is a leaf (caller should open the detail view instead).
    pub fn uc_drill(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        if self.focus != Panel::Catalog {
            return false;
        }
        let Some(item) = self.selected_item() else {
            return self.uc_path.is_empty(); // empty root pane: swallow the key
        };
        // Below the schema level only volumes and their directories are
        // containers; tables/views fall through to the detail view.
        if self.uc_path.len() >= 2 {
            let drillable =
                matches!(&item.status, Status::Unknown(k) if k == "VOLUME" || k == "DIR");
            if !drillable {
                return false;
            }
        }
        self.uc_path.push(item.name.clone());
        self.refresh_catalog(cli);
        true
    }

    /// Ascends one level; returns false if already at the catalog root.
    pub fn uc_up(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        if self.focus != Panel::Catalog || self.uc_path.is_empty() {
            return false;
        }
        self.uc_path.pop();
        self.refresh_catalog(cli);
        true
    }

    fn refresh_catalog(&mut self, cli: &Arc<DatabricksCli>) {
        self.shapes[5] = None;
        self.selected[5] = 0;
        // A filter typed at one level would silently hide the next.
        self.filters[5].clear();
        let (tx, rx) = oneshot::channel();
        self.uc_rx = Some(rx);
        let cli = Arc::clone(cli);
        let path = self.uc_path.clone();
        tokio::spawn(async move {
            let result = fetchers::catalog::fetch(&cli, &path)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
        });
    }

    /// Enter in the Secrets pane: descend from a scope into its keys.
    /// On a key it does nothing — secret values are never displayed —
    /// but still returns true so Enter never opens a (bogus) detail view.
    pub fn secrets_drill(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        if self.focus != Panel::Secrets {
            return false;
        }
        // Already inside a scope: the selection is a key, nothing to open.
        if self.secret_scope.is_some() {
            return true;
        }
        let Some(item) = self.selected_item() else {
            return true; // empty pane: swallow the key
        };
        self.secret_scope = Some(item.name.clone());
        self.refresh_secrets(cli);
        true
    }

    /// Backspace inside a scope: back to the scopes listing.
    pub fn secrets_up(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        if self.focus != Panel::Secrets || self.secret_scope.is_none() {
            return false;
        }
        self.secret_scope = None;
        self.refresh_secrets(cli);
        true
    }

    fn refresh_secrets(&mut self, cli: &Arc<DatabricksCli>) {
        let idx = 6;
        self.shapes[idx] = None;
        self.selected[idx] = 0;
        self.filters[idx].clear();
        let (tx, rx) = oneshot::channel();
        self.secrets_rx = Some(rx);
        let cli = Arc::clone(cli);
        let scope = self.secret_scope.clone();
        tokio::spawn(async move {
            let result = fetchers::secrets::fetch(&cli, scope.as_deref())
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
        });
    }

    pub fn poll_secrets(&mut self) -> bool {
        let Some(rx) = &mut self.secrets_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.shapes[6] = Some(match result {
                    Ok(shape) => shape,
                    Err(e) => Shape::Text(format!("✗ {e}")),
                });
                self.updated_at[6] = Some(Instant::now());
                self.secrets_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.secrets_rx = None;
                true
            }
        }
    }

    /// `a` in the secrets pane: create a scope (top level) or add a
    /// secret (inside a scope).
    pub fn open_secret_form(&mut self) {
        if self.focus != Panel::Secrets {
            return;
        }
        self.secret_form = Some(SecretForm {
            scope: self.secret_scope.clone(),
            key: String::new(),
            value: String::new(),
            stage: 0,
        });
    }

    pub fn secret_form_push(&mut self, c: char) {
        if let Some(form) = &mut self.secret_form {
            if form.stage == 0 {
                form.key.push(c);
            } else {
                form.value.push(c);
            }
        }
    }

    pub fn secret_form_pop(&mut self) {
        if let Some(form) = &mut self.secret_form {
            if form.stage == 0 {
                form.key.pop();
            } else {
                form.value.pop();
            }
        }
    }

    /// Enter in the form: advance to the value stage, or submit.
    pub fn secret_form_submit(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(form) = &mut self.secret_form else {
            return;
        };
        if form.key.trim().is_empty() {
            return;
        }
        match (form.scope.clone(), form.stage) {
            // New scope: single field.
            (None, _) => {
                let name = form.key.trim().to_string();
                self.secret_form = None;
                self.run_secret_action(
                    cli,
                    format!("Create scope “{name}”"),
                    vec!["secrets".into(), "create-scope".into(), name],
                );
            }
            // Secret: key first, then value.
            (Some(_), 0) => form.stage = 1,
            (Some(scope), _) => {
                let key = form.key.trim().to_string();
                let value = form.value.clone();
                self.secret_form = None;
                self.run_secret_action(
                    cli,
                    format!("Put secret “{key}” in “{scope}”"),
                    vec![
                        "secrets".into(),
                        "put-secret".into(),
                        scope,
                        key,
                        "--string-value".into(),
                        value,
                    ],
                );
            }
        }
    }

    /// Runs a secrets mutation right away (the form itself was the
    /// deliberate step); the usual action poll refreshes the panes.
    fn run_secret_action(&mut self, cli: &Arc<DatabricksCli>, label: String, args: Vec<String>) {
        self.flash = Some((format!("⏳ {label}…"), Instant::now()));
        let (tx, rx) = oneshot::channel();
        self.action_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let result = match cli.run_action(&arg_refs).await {
                Ok(()) => Ok(format!("✓ {label} — done")),
                Err(e) => Err(format!("✗ {e:#}")),
            };
            let _ = tx.send(result);
        });
    }

    /// `x` in the secrets pane: delete the selected scope or key.
    pub fn request_secret_delete(&mut self) {
        if self.focus != Panel::Secrets {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        let name = item.name.clone();
        let (message, args) = match &self.secret_scope {
            None => (
                format!("Delete scope “{name}” and all its secrets?"),
                vec!["secrets".to_string(), "delete-scope".to_string(), name],
            ),
            Some(scope) => (
                format!("Delete secret “{name}” from “{scope}”?"),
                vec![
                    "secrets".to_string(),
                    "delete-secret".to_string(),
                    scope.clone(),
                    name,
                ],
            ),
        };
        self.confirm = Some(Confirm { message, args });
    }

    /// `g` in the secrets pane: the scope's ACLs.
    fn open_secret_acls(&mut self, cli: &Arc<DatabricksCli>) {
        let scope = match &self.secret_scope {
            Some(s) => Some(s.clone()),
            None => self.selected_item().map(|i| i.name.clone()),
        };
        let Some(scope) = scope else {
            return;
        };
        self.detail = Some(Detail {
            panel: Panel::Secrets,
            name: scope.clone(),
            id: scope.clone(),
            kind: None,
            section: "Access",
            data: None,
            show_raw: false,
            scroll: 0,
        });
        let (tx, rx) = oneshot::channel();
        self.detail_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let acl_args = ["secrets", "list-acls", &scope];
            let data = match cli.run(&acl_args).await {
                Ok(json) => {
                    let raw =
                        serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string());
                    // The CLI unwraps to a bare array; REST wraps in "items".
                    let acls = json
                        .as_array()
                        .cloned()
                        .or_else(|| json["items"].as_array().cloned())
                        .unwrap_or_default();
                    let activity: Vec<(Status, String)> = acls
                        .iter()
                        .map(|a| {
                            let principal = a["principal"].as_str().unwrap_or("?");
                            let perm = a["permission"].as_str().unwrap_or("?");
                            let status = if perm == "MANAGE" {
                                Status::Success
                            } else {
                                Status::Unknown(String::new())
                            };
                            (status, format!("{principal}  ·  {perm}"))
                        })
                        .collect();
                    DetailData {
                        summary: vec![("Scope".to_string(), scope.clone())],
                        activity,
                        raw,
                    }
                }
                Err(e) => DetailData {
                    summary: Vec::new(),
                    activity: Vec::new(),
                    raw: format!("{e:#}"),
                },
            };
            let _ = tx.send(data);
        });
    }

    /// Looks up a resource's display name by id in the loaded panes —
    /// lets the cost view show "nightly-etl" instead of a job id.
    pub fn resource_name(&self, kind: &str, id: &str) -> Option<String> {
        let idx = match kind {
            "cluster" => 0,
            "job" => 1,
            "warehouse" => 3,
            _ => return None,
        };
        match &self.shapes[idx] {
            Some(Shape::List(items)) => items
                .iter()
                .find(|i| i.id.as_deref() == Some(id))
                .map(|i| i.name.clone()),
            _ => None,
        }
    }

    /// All known warehouses as (name, id, running).
    pub fn warehouses(&self) -> Vec<(String, String, bool)> {
        let Some(Shape::List(items)) = &self.shapes[3] else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|i| {
                let id = i.id.clone()?;
                Some((i.name.clone(), id, matches!(i.status, Status::Running)))
            })
            .collect()
    }

    /// Runs a sample-data query for the selected table or view. With
    /// `force_pick` (or several warehouses and no remembered choice) a
    /// warehouse picker opens first.
    pub fn open_preview(&mut self, cli: &Arc<DatabricksCli>, force_pick: bool) {
        if self.focus != Panel::Catalog {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        if !matches!(&item.status, Status::Unknown(k) if k == "TABLE" || k == "VIEW") {
            return;
        }
        let Some(full_name) = item.id.clone() else {
            return;
        };
        let warehouses = self.warehouses();
        if warehouses.is_empty() {
            self.flash = Some((
                "✗ no SQL warehouse available for previews".to_string(),
                Instant::now(),
            ));
            return;
        }
        if !force_pick {
            if let Some((id, name)) = self.preview_warehouse.clone() {
                self.start_preview_query(cli, full_name, id, name);
                return;
            }
            if let [(name, id, _)] = warehouses.as_slice() {
                self.preview_warehouse = Some((id.clone(), name.clone()));
                self.start_preview_query(cli, full_name, id.clone(), name.clone());
                return;
            }
        }
        // Default the cursor to the remembered choice, else a running warehouse.
        let index = self
            .preview_warehouse
            .as_ref()
            .and_then(|(id, _)| warehouses.iter().position(|(_, wid, _)| wid == id))
            .or_else(|| warehouses.iter().position(|(_, _, running)| *running))
            .unwrap_or(0);
        self.wh_picker = Some(WhPicker {
            index,
            target: PickTarget::Preview(full_name),
        });
    }

    /// Opens the DBU usage view, resolving a warehouse like previews do.
    pub fn open_cost(&mut self, cli: &Arc<DatabricksCli>) {
        let warehouses = self.warehouses();
        if warehouses.is_empty() {
            self.flash = Some((
                "✗ no SQL warehouse available to query system tables".to_string(),
                Instant::now(),
            ));
            return;
        }
        if let Some((id, name)) = self.preview_warehouse.clone() {
            self.start_cost_query(cli, id, name);
            return;
        }
        if let [(name, id, _)] = warehouses.as_slice() {
            self.preview_warehouse = Some((id.clone(), name.clone()));
            self.start_cost_query(cli, id.clone(), name.clone());
            return;
        }
        let index = warehouses
            .iter()
            .position(|(_, _, running)| *running)
            .unwrap_or(0);
        self.wh_picker = Some(WhPicker {
            index,
            target: PickTarget::Cost,
        });
    }

    fn start_cost_query(&mut self, cli: &Arc<DatabricksCli>, id: String, name: String) {
        self.cost = Some(CostView {
            warehouse: name,
            data: None,
        });
        let (tx, rx) = oneshot::channel();
        self.cost_rx = Some(rx);
        let cli = Arc::clone(cli);
        let host = self.host.clone();
        let cached_ws = self.workspace_id.clone();
        tokio::spawn(async move {
            // Scope usage to this workspace; resolved once, then cached.
            let ws = match (cached_ws, host) {
                (Some(w), _) => Some(w),
                (None, Some(h)) => fetchers::cost::resolve_workspace_id(&cli, &id, &h).await,
                (None, None) => None,
            };
            let result = fetchers::cost::fetch(&cli, &id, ws.as_deref()).await;
            let _ = tx.send((result, ws));
        });
    }

    /// Opens the lineage view for the selected table/view; needs a
    /// warehouse since lineage lives in system tables.
    pub fn open_lineage(&mut self, cli: &Arc<DatabricksCli>) {
        if self.focus != Panel::Catalog {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        if !matches!(&item.status, Status::Unknown(k) if k == "TABLE" || k == "VIEW") {
            return;
        }
        let Some(full_name) = item.id.clone() else {
            return;
        };
        let warehouses = self.warehouses();
        if warehouses.is_empty() {
            self.flash = Some((
                "✗ no SQL warehouse available to query lineage".to_string(),
                Instant::now(),
            ));
            return;
        }
        if let Some((id, _)) = self.preview_warehouse.clone() {
            self.start_lineage_query(cli, full_name, id);
            return;
        }
        if let [(name, id, _)] = warehouses.as_slice() {
            self.preview_warehouse = Some((id.clone(), name.clone()));
            let id = id.clone();
            self.start_lineage_query(cli, full_name, id);
            return;
        }
        let index = warehouses
            .iter()
            .position(|(_, _, running)| *running)
            .unwrap_or(0);
        self.wh_picker = Some(WhPicker {
            index,
            target: PickTarget::Lineage(full_name),
        });
    }

    fn start_lineage_query(&mut self, cli: &Arc<DatabricksCli>, full_name: String, wh_id: String) {
        self.detail = Some(Detail {
            panel: Panel::Catalog,
            name: full_name.clone(),
            id: full_name.clone(),
            kind: None,
            section: "Lineage",
            data: None,
            show_raw: false,
            scroll: 0,
        });
        let (tx, rx) = oneshot::channel();
        self.detail_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let data = fetchers::lineage::fetch(&cli, &full_name, &wh_id).await;
            let _ = tx.send(data);
        });
    }

    pub fn close_cost(&mut self) {
        self.cost = None;
        self.cost_rx = None;
    }

    /// The fully-qualified name of the selected catalog-pane table/view.
    fn selected_table_fqn(&self) -> Option<String> {
        if self.focus != Panel::Catalog {
            return None;
        }
        let item = self.selected_item()?;
        if !matches!(&item.status, Status::Unknown(k) if k == "TABLE" || k == "VIEW") {
            return None;
        }
        item.id.clone()
    }

    /// Opens the SQL console. With a table/view selected in the catalog
    /// pane, the prompt starts as an editable query against it.
    pub fn open_sql(&mut self) {
        if self.sql.is_none() {
            let input = self
                .selected_table_fqn()
                .map(|fqn| format!("SELECT * FROM {fqn} LIMIT 100"))
                .unwrap_or_default();
            self.sql = Some(SqlConsole {
                cursor: input.chars().count(),
                input,
                warehouse: String::new(),
                running: false,
                data: None,
                last_sql: String::new(),
                scroll: 0,
                col: 0,
            });
        }
    }

    pub fn close_sql(&mut self) {
        self.sql = None;
        self.sql_rx = None;
        self.hist_idx = None;
        self.hist_draft.clear();
        self.hist_search = None;
        self.sql_complete = None;
    }

    /// Tab in the SQL prompt: complete catalog / schema / table / column
    /// names from the workspace, fetching each level once per session.
    /// A repeat press cycles to the next candidate.
    pub fn sql_tab(&mut self, cli: &Arc<DatabricksCli>) {
        match &self.sql_complete {
            Some(c) if c.loading => return,
            Some(_) => {
                self.sql_complete_next(1);
                return;
            }
            None => {}
        }
        let Some(console) = &self.sql else {
            return;
        };
        let (seg_start, context, prefix) = token_at_cursor(&console.input, console.cursor);
        // Which cache entry is missing: column names of the FROM table
        // matter most for a bare word, then the level being dotted into.
        let missing = if context.is_empty() {
            from_table(&console.input)
                .filter(|fqn| !self.uc_names.contains_key(fqn))
                .or_else(|| (!self.uc_names.contains_key("")).then(String::new))
        } else {
            (!self.uc_names.contains_key(&context)).then(|| context.clone())
        };
        let loading = missing.is_some();
        self.sql_complete = Some(SqlComplete {
            items: Vec::new(),
            index: 0,
            seg_start,
            prefix,
            context,
            loading,
        });
        match missing {
            Some(path) => self.fetch_uc_names(cli, path),
            None => self.sql_complete_fill(),
        }
    }

    /// Builds the candidate list from the cache and inserts the first
    /// match; single matches complete silently, no popup.
    fn sql_complete_fill(&mut self) {
        let Some(console) = &self.sql else {
            self.sql_complete = None;
            return;
        };
        let input = console.input.clone();
        let Some(comp) = &self.sql_complete else {
            return;
        };
        let q = comp.prefix.to_lowercase();
        let mut items: Vec<String> = Vec::new();
        if comp.context.is_empty() {
            if let Some(cols) = from_table(&input).and_then(|fqn| self.uc_names.get(&fqn)) {
                items.extend(
                    cols.iter()
                        .filter(|n| n.to_lowercase().starts_with(&q))
                        .cloned(),
                );
            }
            if let Some(cats) = self.uc_names.get("") {
                items.extend(
                    cats.iter()
                        .filter(|n| n.to_lowercase().starts_with(&q))
                        .cloned(),
                );
            }
            if !q.is_empty() {
                items.extend(
                    SQL_KEYWORDS
                        .iter()
                        .filter(|k| k.to_lowercase().starts_with(&q))
                        .map(|k| k.to_string()),
                );
            }
        } else if let Some(names) = self.uc_names.get(&comp.context) {
            items.extend(
                names
                    .iter()
                    .filter(|n| n.to_lowercase().starts_with(&q))
                    .cloned(),
            );
        }
        items.dedup();
        if items.is_empty() {
            self.sql_complete = None;
            self.flash = Some(("no completions".to_string(), Instant::now()));
            return;
        }
        let single = items.len() == 1;
        if let Some(comp) = &mut self.sql_complete {
            comp.items = items;
            comp.index = 0;
        }
        self.sql_complete_apply();
        if single {
            self.sql_complete = None;
        }
    }

    /// Replaces the completed segment with the current candidate.
    fn sql_complete_apply(&mut self) {
        let Some(comp) = &self.sql_complete else {
            return;
        };
        let candidate = comp.items[comp.index].clone();
        let plain = candidate
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | ' '));
        let text = if plain {
            candidate
        } else {
            format!("`{candidate}`")
        };
        self.sql_replace_segment(comp.seg_start, &text);
    }

    /// Overwrites seg_start..caret with `text`, caret landing at its end.
    fn sql_replace_segment(&mut self, seg_start: usize, text: &str) {
        if let Some(console) = &mut self.sql {
            let from = byte_at(&console.input, seg_start);
            let to = byte_at(&console.input, console.cursor);
            console.input.replace_range(from..to, text);
            console.cursor = seg_start + text.chars().count();
        }
    }

    /// Tab / shift+tab with the popup open: cycle candidates.
    pub fn sql_complete_next(&mut self, delta: i32) {
        let Some(comp) = &mut self.sql_complete else {
            return;
        };
        if comp.items.is_empty() {
            return;
        }
        let n = comp.items.len() as i32;
        comp.index = (comp.index as i32 + delta).rem_euclid(n) as usize;
        self.sql_complete_apply();
    }

    /// Esc: restore the typed prefix and close the popup.
    pub fn sql_complete_cancel(&mut self) {
        if let Some(comp) = &self.sql_complete {
            let (seg_start, prefix) = (comp.seg_start, comp.prefix.clone());
            self.sql_replace_segment(seg_start, &prefix);
        }
        self.sql_complete = None;
    }

    /// Keeps the inserted candidate and closes the popup.
    pub fn sql_complete_accept(&mut self) {
        self.sql_complete = None;
    }

    fn fetch_uc_names(&mut self, cli: &Arc<DatabricksCli>, path: String) {
        let (tx, rx) = oneshot::channel();
        self.uc_names_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let names = fetchers::catalog::names(&cli, &path)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send((path, names));
        });
    }

    /// Caches fetched completion names and fills the waiting popup.
    pub fn poll_uc_names(&mut self) -> bool {
        let Some(rx) = &mut self.uc_names_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok((path, result)) => {
                self.uc_names_rx = None;
                match result {
                    Ok(names) => {
                        self.uc_names.insert(path, names);
                        if self.sql_complete.as_ref().is_some_and(|c| c.loading) {
                            if let Some(c) = &mut self.sql_complete {
                                c.loading = false;
                            }
                            self.sql_complete_fill();
                        }
                    }
                    Err(e) => {
                        self.sql_complete = None;
                        let first = e.lines().next().unwrap_or("fetch failed").to_string();
                        self.flash = Some((format!("✗ completions: {first}"), Instant::now()));
                    }
                }
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.uc_names_rx = None;
                self.sql_complete = None;
                true
            }
        }
    }

    /// The statement currently in the prompt.
    pub fn sql_input(&self) -> Option<String> {
        self.sql.as_ref().map(|c| c.input.clone())
    }

    /// Replaces the prompt contents (after an $EDITOR round-trip).
    pub fn sql_set_input(&mut self, s: &str) {
        if let Some(console) = &mut self.sql {
            console.input = s.to_string();
            console.cursor = console.input.chars().count();
        }
    }

    /// The history entry the active Ctrl+R search currently matches.
    pub fn hist_search_current(&self) -> Option<&String> {
        let (query, nth) = self.hist_search.as_ref()?;
        self.sql_history
            .iter()
            .rev()
            .filter(|h| h.to_lowercase().contains(&query.to_lowercase()))
            .nth(*nth)
    }

    pub fn hist_search_start(&mut self) {
        if self.sql.is_some() {
            self.hist_search = Some((String::new(), 0));
        }
    }

    pub fn hist_search_push(&mut self, c: char) {
        if let Some((query, nth)) = &mut self.hist_search {
            query.push(c);
            *nth = 0;
        }
    }

    pub fn hist_search_pop(&mut self) {
        if let Some((query, nth)) = &mut self.hist_search {
            query.pop();
            *nth = 0;
        }
    }

    /// Ctrl+R again: step to the next older match.
    pub fn hist_search_older(&mut self) {
        let Some((query, nth)) = &self.hist_search else {
            return;
        };
        let q = query.to_lowercase();
        let matches = self
            .sql_history
            .iter()
            .filter(|h| h.to_lowercase().contains(&q))
            .count();
        if nth + 1 < matches {
            if let Some((_, n)) = &mut self.hist_search {
                *n += 1;
            }
        }
    }

    pub fn hist_search_accept(&mut self) {
        if let Some(stmt) = self.hist_search_current().cloned() {
            self.sql_set_input(&stmt);
        }
        self.hist_search = None;
    }

    pub fn hist_search_cancel(&mut self) {
        self.hist_search = None;
    }

    pub fn sql_push(&mut self, c: char) {
        if let Some(console) = &mut self.sql {
            let at = byte_at(&console.input, console.cursor);
            console.input.insert(at, c);
            console.cursor += 1;
        }
    }

    /// Backspace: deletes the character before the caret.
    pub fn sql_pop(&mut self) {
        if let Some(console) = &mut self.sql {
            if console.cursor > 0 {
                let at = byte_at(&console.input, console.cursor - 1);
                console.input.remove(at);
                console.cursor -= 1;
            }
        }
    }

    /// Delete: removes the character under the caret.
    pub fn sql_delete(&mut self) {
        if let Some(console) = &mut self.sql {
            if console.cursor < console.input.chars().count() {
                let at = byte_at(&console.input, console.cursor);
                console.input.remove(at);
            }
        }
    }

    pub fn sql_left(&mut self) {
        if let Some(console) = &mut self.sql {
            console.cursor = console.cursor.saturating_sub(1);
        }
    }

    pub fn sql_right(&mut self) {
        if let Some(console) = &mut self.sql {
            console.cursor = (console.cursor + 1).min(console.input.chars().count());
        }
    }

    /// ↑ at the prompt: step back through history, stashing the draft.
    pub fn sql_hist_prev(&mut self) {
        let Some(console) = &mut self.sql else {
            return;
        };
        if self.sql_history.is_empty() {
            return;
        }
        let idx = match self.hist_idx {
            None => {
                self.hist_draft = console.input.clone();
                self.sql_history.len() - 1
            }
            Some(i) => i.saturating_sub(1),
        };
        self.hist_idx = Some(idx);
        console.input = self.sql_history[idx].clone();
        console.cursor = console.input.chars().count();
    }

    /// ↓ at the prompt: step forward, back to the stashed draft at the end.
    pub fn sql_hist_next(&mut self) {
        let Some(console) = &mut self.sql else {
            return;
        };
        let Some(idx) = self.hist_idx else {
            return;
        };
        if idx + 1 < self.sql_history.len() {
            self.hist_idx = Some(idx + 1);
            console.input = self.sql_history[idx + 1].clone();
        } else {
            self.hist_idx = None;
            console.input = self.hist_draft.clone();
        }
        console.cursor = console.input.chars().count();
    }

    pub fn sql_home(&mut self) {
        if let Some(console) = &mut self.sql {
            console.cursor = 0;
        }
    }

    pub fn sql_end(&mut self) {
        if let Some(console) = &mut self.sql {
            console.cursor = console.input.chars().count();
        }
    }

    pub fn sql_scroll(&mut self, delta: i32) {
        if let Some(console) = &mut self.sql {
            let max = match &console.data {
                Some(Ok(t)) => t.rows.len().saturating_sub(1),
                _ => 0,
            };
            console.scroll = if delta < 0 {
                console.scroll.saturating_sub(delta.unsigned_abs() as usize)
            } else {
                (console.scroll + delta as usize).min(max)
            };
        }
    }

    /// Shift+←/→ in the console: page result columns.
    pub fn sql_cols(&mut self, delta: i32) {
        if let Some(console) = &mut self.sql {
            let n = match &console.data {
                Some(Ok(t)) => t.headers.len(),
                _ => 0,
            };
            console.col = if delta < 0 {
                console.col.saturating_sub(1)
            } else {
                (console.col + 1).min(n.saturating_sub(1))
            };
        }
    }

    /// Runs the typed statement, resolving a warehouse like previews do.
    pub fn sql_run(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(console) = &self.sql else {
            return;
        };
        if console.running {
            return;
        }
        let query = console.input.trim().to_string();
        if query.is_empty() {
            return;
        }
        // Remember the statement (skipping immediate repeats) and reset
        // any in-progress history browsing.
        if self.sql_history.last() != Some(&query) {
            self.sql_history.push(query.clone());
            save_history(&self.sql_history);
        }
        self.hist_idx = None;
        self.hist_draft.clear();
        let warehouses = self.warehouses();
        if warehouses.is_empty() {
            self.flash = Some(("✗ no SQL warehouse available".to_string(), Instant::now()));
            return;
        }
        if let Some((id, name)) = self.preview_warehouse.clone() {
            self.start_sql_query(cli, query, id, name);
            return;
        }
        if let [(name, id, _)] = warehouses.as_slice() {
            self.preview_warehouse = Some((id.clone(), name.clone()));
            self.start_sql_query(cli, query, id.clone(), name.clone());
            return;
        }
        let index = warehouses
            .iter()
            .position(|(_, _, running)| *running)
            .unwrap_or(0);
        self.wh_picker = Some(WhPicker {
            index,
            target: PickTarget::Sql(query),
        });
    }

    fn start_sql_query(
        &mut self,
        cli: &Arc<DatabricksCli>,
        query: String,
        id: String,
        name: String,
    ) {
        if let Some(console) = &mut self.sql {
            console.running = true;
            console.warehouse = name;
            console.scroll = 0;
            console.last_sql = query.clone();
        }
        // Published by the task once submitted, so Esc can cancel it.
        let handle = std::sync::Arc::new(std::sync::Mutex::new(None));
        self.sql_stmt = Some(std::sync::Arc::clone(&handle));
        let (tx, rx) = oneshot::channel();
        self.sql_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let result = fetchers::preview::run_sql_tracked(&cli, &query, &id, Some(handle)).await;
            let _ = tx.send(result);
        });
    }

    /// Writes a result set to a timestamped CSV in the working directory
    /// and flashes the path.
    fn export_csv(&mut self, label: &str, data: &crate::shape::TableData) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let slug: String = label
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .chars()
            .take(40)
            .collect();
        let name = format!("databricks-{slug}-{stamp}.csv");
        let msg = match std::fs::write(&name, data.to_csv()) {
            Ok(()) => {
                let cwd = std::env::current_dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                format!("✓ exported {} rows to {cwd}/{name}", data.rows.len())
            }
            Err(e) => format!("✗ export failed: {e}"),
        };
        self.flash = Some((msg, Instant::now()));
    }

    /// Ctrl+S in the console: export the current results.
    pub fn sql_export(&mut self) {
        if let Some(SqlConsole {
            data: Some(Ok(data)),
            last_sql,
            ..
        }) = &self.sql
        {
            let (label, data) = (last_sql.clone(), data.clone());
            self.export_csv(&label, &data);
        }
    }

    /// ←/→ in a preview: page columns in the grid, switch rows in
    /// record view.
    pub fn preview_h(&mut self, delta: i32) {
        let Some(pv) = &mut self.preview else {
            return;
        };
        if pv.record {
            let max = match &pv.data {
                Some(Ok(t)) => t.rows.len().saturating_sub(1),
                _ => 0,
            };
            pv.scroll = if delta < 0 {
                pv.scroll.saturating_sub(1)
            } else {
                (pv.scroll + 1).min(max)
            };
            pv.rscroll = 0;
        } else {
            let cols = pv.visible_cols().len();
            pv.col = if delta < 0 {
                pv.col.saturating_sub(1)
            } else {
                (pv.col + 1).min(cols.saturating_sub(1))
            };
        }
    }

    /// `v`/enter in a preview: transposed view of the top visible row.
    pub fn preview_toggle_record(&mut self) {
        if let Some(pv) = &mut self.preview {
            if matches!(&pv.data, Some(Ok(t)) if !t.rows.is_empty()) {
                pv.record = !pv.record;
                pv.rscroll = 0;
            }
        }
    }

    pub fn preview_filter_start(&mut self) {
        if let Some(pv) = &mut self.preview {
            pv.filter.clear();
            pv.filter_entry = true;
            pv.col = 0;
            pv.rscroll = 0;
        }
    }

    pub fn preview_filter_push(&mut self, c: char) {
        if let Some(pv) = &mut self.preview {
            pv.filter.push(c);
            pv.col = 0;
            pv.rscroll = 0;
        }
    }

    pub fn preview_filter_pop(&mut self) {
        if let Some(pv) = &mut self.preview {
            pv.filter.pop();
            pv.col = 0;
        }
    }

    pub fn preview_filter_accept(&mut self) {
        if let Some(pv) = &mut self.preview {
            pv.filter_entry = false;
        }
    }

    pub fn preview_filter_clear(&mut self) {
        if let Some(pv) = &mut self.preview {
            pv.filter.clear();
            pv.filter_entry = false;
            pv.col = 0;
            pv.rscroll = 0;
        }
    }

    /// `e` in a table preview: export the sampled rows.
    pub fn preview_export(&mut self) {
        if let Some(Preview {
            data: Some(Ok(data)),
            name,
            ..
        }) = &self.preview
        {
            let (label, data) = (name.clone(), data.clone());
            self.export_csv(&label, &data);
        }
    }

    pub fn poll_sql(&mut self) -> bool {
        let Some(rx) = &mut self.sql_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                // A warehouse that errors shouldn't stay the session
                // default — but a user-canceled statement is not its fault.
                if let Err(e) = &result {
                    if e != "statement canceled" {
                        self.preview_warehouse = None;
                    }
                }
                if let Some(console) = &mut self.sql {
                    console.running = false;
                    console.data = Some(result);
                    console.col = 0;
                }
                self.sql_rx = None;
                self.sql_stmt = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                if let Some(console) = &mut self.sql {
                    console.running = false;
                }
                self.sql_rx = None;
                true
            }
        }
    }

    pub fn poll_cost(&mut self) -> bool {
        let Some(rx) = &mut self.cost_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok((result, ws)) => {
                if result.is_err() {
                    self.preview_warehouse = None;
                }
                if ws.is_some() {
                    self.workspace_id = ws;
                }
                if let Some(cv) = &mut self.cost {
                    cv.data = Some(result);
                }
                self.cost_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.cost_rx = None;
                true
            }
        }
    }

    pub fn wh_picker_next(&mut self) {
        let len = self.warehouses().len();
        if let Some(p) = &mut self.wh_picker {
            p.index = (p.index + 1).min(len.saturating_sub(1));
        }
    }

    pub fn wh_picker_prev(&mut self) {
        if let Some(p) = &mut self.wh_picker {
            p.index = p.index.saturating_sub(1);
        }
    }

    pub fn wh_picker_cancel(&mut self) {
        self.wh_picker = None;
    }

    /// Confirms the warehouse choice, remembers it, and starts the preview.
    pub fn wh_picker_select(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(picker) = self.wh_picker.take() else {
            return;
        };
        let warehouses = self.warehouses();
        let Some((name, id, _)) = warehouses.get(picker.index) else {
            return;
        };
        self.preview_warehouse = Some((id.clone(), name.clone()));
        // An explicit choice is worth remembering across sessions.
        let profile = self.profile.clone().unwrap_or_else(|| "DEFAULT".into());
        self.config
            .warehouses
            .insert(profile, (id.clone(), name.clone()));
        self.config.save();
        match picker.target {
            PickTarget::Preview(table) => {
                self.start_preview_query(cli, table, id.clone(), name.clone())
            }
            PickTarget::Cost => self.start_cost_query(cli, id.clone(), name.clone()),
            PickTarget::Lineage(table) => self.start_lineage_query(cli, table, id.clone()),
            PickTarget::Sql(query) => self.start_sql_query(cli, query, id.clone(), name.clone()),
        }
    }

    fn start_preview_query(
        &mut self,
        cli: &Arc<DatabricksCli>,
        full_name: String,
        warehouse_id: String,
        warehouse_name: String,
    ) {
        self.preview = Some(Preview {
            name: full_name.clone(),
            warehouse: warehouse_name,
            warehouse_id: warehouse_id.clone(),
            data: None,
            scroll: 0,
            col: 0,
            filter: String::new(),
            filter_entry: false,
            record: false,
            rscroll: 0,
        });
        let (tx, rx) = oneshot::channel();
        self.preview_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let result = fetchers::preview::fetch(&cli, &full_name, &warehouse_id).await;
            let _ = tx.send(result);
        });
    }

    pub fn close_preview(&mut self) {
        self.preview = None;
        self.preview_rx = None;
    }

    pub fn poll_preview(&mut self) -> bool {
        let Some(rx) = &mut self.preview_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                // A warehouse that errors shouldn't stay the session default.
                if result.is_err() {
                    self.preview_warehouse = None;
                }
                if let Some(pv) = &mut self.preview {
                    pv.data = Some(result);
                }
                self.preview_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.preview_rx = None;
                true
            }
        }
    }

    pub fn preview_scroll(&mut self, delta: i32) {
        if let Some(pv) = &mut self.preview {
            // Record view: j/k walk the fields, not the rows.
            if pv.record {
                let max = pv.visible_cols().len().saturating_sub(1) as u16;
                pv.rscroll = if delta < 0 {
                    pv.rscroll.saturating_sub(delta.unsigned_abs() as u16)
                } else {
                    pv.rscroll.saturating_add(delta as u16).min(max)
                };
                return;
            }
            let max = match &pv.data {
                Some(Ok(t)) => t.rows.len().saturating_sub(1),
                _ => 0,
            };
            pv.scroll = if delta < 0 {
                pv.scroll.saturating_sub(delta.unsigned_abs() as usize)
            } else {
                (pv.scroll + delta as usize).min(max)
            };
        }
    }

    pub fn poll_uc(&mut self) -> bool {
        let Some(rx) = &mut self.uc_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.shapes[5] = Some(match result {
                    Ok(shape) => shape,
                    Err(e) => Shape::Text(format!("✗ {e}")),
                });
                self.updated_at[5] = Some(Instant::now());
                self.uc_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.uc_rx = None;
                true
            }
        }
    }

    /// Opens the access view for the selected item: effective UC grants
    /// or the workspace object ACL.
    pub fn open_grants(&mut self, cli: &Arc<DatabricksCli>) {
        if self.focus == Panel::Secrets {
            return self.open_secret_acls(cli);
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(id) = item.id.clone() else {
            return;
        };
        let (uc, object_type): (bool, &'static str) = match self.focus {
            Panel::Catalog => match &item.status {
                Status::Unknown(k) if k == "CATALOG" => (true, "catalog"),
                Status::Unknown(k) if k == "SCHEMA" => (true, "schema"),
                Status::Unknown(k) if k == "TABLE" || k == "VIEW" => (true, "table"),
                Status::Unknown(k) if k == "VOLUME" => (true, "volume"),
                _ => return,
            },
            Panel::Clusters => (false, "clusters"),
            Panel::Jobs => (false, "jobs"),
            Panel::Pipelines => (false, "pipelines"),
            Panel::Warehouses => (false, "warehouses"),
            Panel::Dashboards => (false, "dashboards"),
            // Secrets ACLs are handled by open_secret_acls above.
            Panel::Secrets => return,
        };
        self.detail = Some(Detail {
            panel: self.focus,
            name: item.name.clone(),
            id: id.clone(),
            kind: None,
            section: "Access",
            data: None,
            show_raw: false,
            scroll: 0,
        });
        let (tx, rx) = oneshot::channel();
        self.detail_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let data = fetchers::grants::fetch(&cli, uc, object_type, &id).await;
            let _ = tx.send(data);
        });
    }

    /// Drills from an open job or pipeline detail into its most recent
    /// run/update.
    pub fn open_run(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(d) = &self.detail else {
            return;
        };
        let panel = d.panel;
        if !matches!(panel, Panel::Jobs | Panel::Pipelines) || d.section == "Lineage" {
            return;
        }
        let owner_id = d.id.clone();
        self.run_view = Some(RunView {
            panel,
            owner_name: d.name.clone(),
            owner_id: owner_id.clone(),
            runs: Vec::new(),
            idx: 0,
            data: None,
            show_raw: false,
            scroll: 0,
            live: false,
            output: None,
            show_output: false,
            show_timeline: false,
            show_dag: false,
            show_grid: false,
            grid: None,
            fetched_at: Instant::now(),
        });
        let (tx, rx) = oneshot::channel();
        self.run_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let result = async {
                let runs = if panel == Panel::Jobs {
                    fetchers::runs::list(&cli, &owner_id).await?
                } else {
                    fetchers::updates::list(&cli, &owner_id).await?
                };
                let Some((run_id, _, _)) = runs.first().cloned() else {
                    return Err("no runs recorded yet".to_string());
                };
                let (data, live) = if panel == Panel::Jobs {
                    fetchers::runs::fetch(&cli, &run_id).await
                } else {
                    fetchers::updates::fetch(&cli, &owner_id, &run_id).await
                };
                Ok((runs, data, live))
            }
            .await;
            let _ = tx.send(RunUpdate::Opened(result));
        });
    }

    pub fn close_run(&mut self) {
        self.run_view = None;
        self.run_rx = None;
        self.grid_rx = None;
    }

    /// Moves to an older (delta > 0) or newer (delta < 0) run.
    pub fn run_nav(&mut self, cli: &Arc<DatabricksCli>, delta: i32) {
        if self.run_rx.is_some() {
            return;
        }
        let Some(rv) = &mut self.run_view else {
            return;
        };
        if rv.runs.is_empty() {
            return;
        }
        let new = if delta < 0 {
            rv.idx.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (rv.idx + delta as usize).min(rv.runs.len() - 1)
        };
        if new == rv.idx {
            return;
        }
        rv.idx = new;
        rv.data = None;
        rv.scroll = 0;
        rv.show_raw = false;
        rv.output = None;
        rv.show_output = false;
        let run_id = rv.runs[new].0.clone();
        self.start_run_fetch(cli, run_id);
    }

    fn start_run_fetch(&mut self, cli: &Arc<DatabricksCli>, run_id: String) {
        let Some(rv) = &self.run_view else {
            return;
        };
        let (panel, owner_id) = (rv.panel, rv.owner_id.clone());
        let (tx, rx) = oneshot::channel();
        self.run_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let (data, live) = if panel == Panel::Jobs {
                fetchers::runs::fetch(&cli, &run_id).await
            } else {
                fetchers::updates::fetch(&cli, &owner_id, &run_id).await
            };
            let _ = tx.send(RunUpdate::Detail(data, live));
        });
    }

    /// `o` in the run view: toggles the full output/log view, fetching
    /// all task outputs on first use.
    pub fn run_toggle_output(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(rv) = &mut self.run_view else {
            return;
        };
        if rv.panel != Panel::Jobs {
            self.flash = Some((
                "✗ output view is for job runs — pipeline events are already inline".to_string(),
                Instant::now(),
            ));
            return;
        }
        if rv.show_output {
            rv.show_output = false;
            rv.scroll = 0;
            return;
        }
        if rv.output.is_none() && self.run_rx.is_some() {
            self.flash = Some((
                "⏳ run still loading — try again in a moment".to_string(),
                Instant::now(),
            ));
            return;
        }
        rv.show_output = true;
        rv.scroll = 0;
        if rv.output.is_some() {
            return;
        }
        self.start_output_fetch(cli);
    }

    fn start_output_fetch(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(rv) = &self.run_view else {
            return;
        };
        let Some((run_id, _, _)) = rv.runs.get(rv.idx).cloned() else {
            return;
        };
        let (tx, rx) = oneshot::channel();
        self.run_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let (text, live) = fetchers::runs::full_output(&cli, &run_id).await;
            let _ = tx.send(RunUpdate::Output(text, live));
        });
    }

    /// `r` in the run view: rerun only the failed tasks of the shown run.
    pub fn request_run_repair(&mut self) {
        let Some(rv) = &self.run_view else {
            return;
        };
        if rv.panel != Panel::Jobs {
            self.flash = Some((
                "✗ repair applies to job runs only".to_string(),
                Instant::now(),
            ));
            return;
        }
        if rv.live {
            self.flash = Some((
                "✗ run is still executing — cancel it first (s)".to_string(),
                Instant::now(),
            ));
            return;
        }
        let Some((run_id, status, _)) = rv.runs.get(rv.idx) else {
            return;
        };
        if matches!(status, Status::Success) {
            self.flash = Some((
                "✗ run succeeded — nothing to repair".to_string(),
                Instant::now(),
            ));
            return;
        }
        self.confirm = Some(Confirm {
            message: format!(
                "Repair run {run_id} of “{}” (reruns only the failed tasks)?",
                rv.owner_name
            ),
            args: vec![
                "jobs".to_string(),
                "repair-run".to_string(),
                run_id.clone(),
                "--rerun-all-failed-tasks".to_string(),
            ],
        });
    }

    /// `t` in the run view: per-task execution timeline of a job run.
    pub fn run_toggle_timeline(&mut self) {
        let Some(rv) = &mut self.run_view else {
            return;
        };
        if rv.panel != Panel::Jobs {
            self.flash = Some((
                "✗ timeline is for job runs — pipeline events are already inline".to_string(),
                Instant::now(),
            ));
            return;
        }
        rv.show_timeline = !rv.show_timeline;
        rv.show_dag = false;
        rv.show_grid = false;
        rv.scroll = 0;
    }

    /// `d` in the run view: dependency tree of the run's tasks.
    pub fn run_toggle_dag(&mut self) {
        let Some(rv) = &mut self.run_view else {
            return;
        };
        if rv.panel != Panel::Jobs {
            self.flash = Some((
                "✗ the task DAG is for job runs — pipelines have no task graph here".to_string(),
                Instant::now(),
            ));
            return;
        }
        rv.show_dag = !rv.show_dag;
        rv.show_timeline = false;
        rv.show_grid = false;
        rv.scroll = 0;
    }

    /// `g` in the run view: history grid — every task's state across the
    /// job's recent runs, with duration trends.
    pub fn run_toggle_grid(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(rv) = &mut self.run_view else {
            return;
        };
        if rv.panel != Panel::Jobs {
            self.flash = Some((
                "✗ the history grid is for job runs — updates have no task matrix".to_string(),
                Instant::now(),
            ));
            return;
        }
        rv.show_grid = !rv.show_grid;
        rv.show_timeline = false;
        rv.show_dag = false;
        rv.scroll = 0;
        if !rv.show_grid || rv.grid.is_some() || self.grid_rx.is_some() {
            return;
        }
        let job_id = rv.owner_id.clone();
        let (tx, rx) = oneshot::channel();
        self.grid_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let _ = tx.send(fetchers::runs::grid(&cli, &job_id).await);
        });
    }

    pub fn poll_grid(&mut self) -> bool {
        let Some(rx) = &mut self.grid_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.grid_rx = None;
                if let Some(rv) = &mut self.run_view {
                    rv.grid = Some(result);
                }
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.grid_rx = None;
                true
            }
        }
    }

    pub fn run_toggle_raw(&mut self) {
        if let Some(rv) = &mut self.run_view {
            rv.show_raw = !rv.show_raw;
            rv.scroll = 0;
        }
    }

    pub fn run_scroll(&mut self, delta: i32) {
        if let Some(rv) = &mut self.run_view {
            rv.scroll = if delta < 0 {
                rv.scroll.saturating_sub(delta.unsigned_abs() as u16)
            } else {
                rv.scroll.saturating_add(delta as u16)
            };
        }
    }

    /// Applies run fetch results; also re-polls a live run every few
    /// seconds so an executing run's tasks update on their own.
    pub fn poll_run(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        if let Some(rx) = &mut self.run_rx {
            match rx.try_recv() {
                Ok(update) => {
                    self.run_rx = None;
                    if let Some(rv) = &mut self.run_view {
                        match update {
                            RunUpdate::Opened(Ok((runs, data, live))) => {
                                rv.runs = runs;
                                rv.idx = 0;
                                rv.data = Some(data);
                                rv.live = live;
                            }
                            RunUpdate::Opened(Err(e)) => {
                                rv.data = Some(DetailData {
                                    summary: Vec::new(),
                                    activity: Vec::new(),
                                    raw: format!("✗ {e}"),
                                });
                                rv.live = false;
                            }
                            RunUpdate::Detail(data, live) => {
                                rv.data = Some(data);
                                rv.live = live;
                            }
                            RunUpdate::Output(text, live) => {
                                rv.output = Some(text);
                                rv.live = live;
                            }
                        }
                        rv.fetched_at = Instant::now();
                    }
                    true
                }
                Err(oneshot::error::TryRecvError::Empty) => false,
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.run_rx = None;
                    true
                }
            }
        } else if let Some(rv) = &self.run_view {
            if rv.live && rv.data.is_some() && rv.fetched_at.elapsed() >= Duration::from_secs(5) {
                if rv.show_output {
                    // Live tail: keep re-fetching output so task results
                    // stream in as they finish.
                    if rv.output.is_some() {
                        self.start_output_fetch(cli);
                    }
                } else if let Some((run_id, _, _)) = rv.runs.get(rv.idx).cloned() {
                    self.start_run_fetch(cli, run_id);
                }
            }
            false
        } else {
            false
        }
    }

    pub fn close_detail(&mut self) {
        self.detail = None;
        self.detail_rx = None;
    }

    pub fn toggle_raw(&mut self) {
        if let Some(d) = &mut self.detail {
            d.show_raw = !d.show_raw;
            d.scroll = 0;
        }
    }

    /// Applies a finished detail fetch; returns true if the UI should redraw.
    pub fn poll_detail(&mut self) -> bool {
        let Some(rx) = &mut self.detail_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(data) => {
                if let Some(d) = &mut self.detail {
                    d.data = Some(data);
                }
                self.detail_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.detail_rx = None;
                true
            }
        }
    }

    pub fn detail_scroll(&mut self, delta: i32) {
        if let Some(d) = &mut self.detail {
            let max = match &d.data {
                Some(data) if d.show_raw => data.raw.lines().count(),
                Some(data) => data.summary.len() + data.activity.len() + 3,
                None => 0,
            } as u16;
            d.scroll = if delta < 0 {
                d.scroll.saturating_sub(delta.unsigned_abs() as u16)
            } else {
                (d.scroll + delta as u16).min(max.saturating_sub(1))
            };
        }
    }

    /// Prepares a contextual action for the selected item, pending confirmation:
    /// start/stop for clusters, warehouses and pipelines, run-now for jobs.
    pub fn request_action(&mut self) {
        // Dashboards, Unity Catalog and secrets have no start/stop/run semantics.
        if matches!(
            self.focus,
            Panel::Dashboards | Panel::Catalog | Panel::Secrets
        ) {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(id) = item.id.clone() else {
            return;
        };
        let name = item.name.clone();
        let active = matches!(
            item.status,
            Status::Running | Status::Pending | Status::Success
        );
        let group = self.focus.cli_group();
        let (verb, action): (&str, &str) = match self.focus {
            Panel::Jobs => ("Run", "run-now"),
            Panel::Clusters if active => ("Stop", "delete"),
            Panel::Pipelines if active => ("Stop", "stop"),
            Panel::Pipelines => ("Start update for", "start-update"),
            _ if active => ("Stop", "stop"),
            _ => ("Start", "start"),
        };
        self.confirm = Some(Confirm {
            message: format!("{verb} {} “{}”?", group.trim_end_matches('s'), name),
            args: vec![group.to_string(), action.to_string(), id],
        });
    }

    /// `S` on the jobs pane: pause or resume the selected job's schedule,
    /// trigger or continuous mode. No confirm — it's a symmetric toggle,
    /// undone by pressing S again.
    pub fn request_schedule_toggle(&mut self, cli: &Arc<DatabricksCli>) {
        if self.focus != Panel::Jobs {
            self.flash = Some((
                "✗ schedule pause applies to jobs — focus the Lakeflow pane".to_string(),
                Instant::now(),
            ));
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(id) = item.id.clone() else {
            return;
        };
        let name = item.name.clone();
        if self.action_rx.is_some() {
            self.flash = Some((
                "⏳ another action is still in flight".to_string(),
                Instant::now(),
            ));
            return;
        }
        self.flash = Some((format!("⏳ toggling schedule of “{name}”…"), Instant::now()));
        let (tx, rx) = oneshot::channel();
        self.action_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let _ = tx.send(fetchers::jobs::toggle_pause(&cli, &id, &name).await);
        });
    }

    /// `s` in the run view: cancel the shown run/update after a confirm.
    pub fn request_run_cancel(&mut self) {
        let Some(rv) = &self.run_view else {
            return;
        };
        if !rv.live {
            self.flash = Some((
                "✗ nothing to cancel — this run already finished".to_string(),
                Instant::now(),
            ));
            return;
        }
        let Some((run_id, _, _)) = rv.runs.get(rv.idx) else {
            return;
        };
        let (message, args) = if rv.panel == Panel::Jobs {
            (
                format!("Cancel run {run_id} of “{}”?", rv.owner_name),
                vec!["jobs".to_string(), "cancel-run".to_string(), run_id.clone()],
            )
        } else {
            (
                format!("Stop “{}” (cancels the active update)?", rv.owner_name),
                vec![
                    "pipelines".to_string(),
                    "stop".to_string(),
                    rv.owner_id.clone(),
                ],
            )
        };
        self.confirm = Some(Confirm { message, args });
    }

    /// Cancels the in-flight console statement server-side; the polling
    /// task then sees CANCELED and surfaces it in the results pane.
    pub fn sql_cancel(&mut self, cli: &Arc<DatabricksCli>) {
        let id = self
            .sql_stmt
            .as_ref()
            .and_then(|h| h.lock().ok().and_then(|g| g.clone()));
        let Some(id) = id else {
            self.flash = Some((
                "✗ statement not submitted yet — try again in a moment".to_string(),
                Instant::now(),
            ));
            return;
        };
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let path = format!("/api/2.0/sql/statements/{id}/cancel");
            let _ = cli.run_action(&["api", "post", &path]).await;
        });
        self.flash = Some(("⏳ cancel requested".to_string(), Instant::now()));
    }

    pub fn cancel_confirm(&mut self) {
        self.confirm = None;
    }

    pub fn confirm_execute(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(c) = self.confirm.take() else {
            return;
        };
        let base = c.message.trim_end_matches('?').to_string();
        self.flash = Some((format!("⏳ {base}…"), Instant::now()));

        let (tx, rx) = oneshot::channel();
        self.action_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let args: Vec<&str> = c.args.iter().map(String::as_str).collect();
            let result = match cli.run_action(&args).await {
                Ok(()) => Ok(format!("✓ {base} — done")),
                Err(e) => Err(format!("✗ {e:#}")),
            };
            let _ = tx.send(result);
        });
    }

    /// Applies a finished action; refreshes on success. Returns true on change.
    pub fn poll_action(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        let Some(rx) = &mut self.action_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                let ok = result.is_ok();
                self.flash = Some((result.unwrap_or_else(|e| e), Instant::now()));
                self.action_rx = None;
                if ok {
                    self.start_refresh(cli);
                    // A confirmed action from the run view (cancel/repair)
                    // changes the shown run — reflect it without a manual nav.
                    if self.run_rx.is_none() {
                        let current = self
                            .run_view
                            .as_ref()
                            .and_then(|rv| rv.runs.get(rv.idx).cloned());
                        if let Some((run_id, _, _)) = current {
                            self.start_run_fetch(cli, run_id);
                        }
                    }
                }
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.action_rx = None;
                true
            }
        }
    }

    /// Drops the flash message once it has been visible long enough.
    pub fn expire_flash(&mut self) -> bool {
        if let Some((_, since)) = &self.flash {
            if since.elapsed() >= Duration::from_secs(5) && self.action_rx.is_none() {
                self.flash = None;
                return true;
            }
        }
        false
    }

    /// Opens the selected item (or the open detail view) in the workspace web UI.
    pub fn open_in_browser(&self) {
        let Some(host) = &self.host else {
            return;
        };
        let (panel, id) = match &self.detail {
            Some(d) => (d.panel, Some(d.id.clone())),
            None => (self.focus, self.selected_item().and_then(|i| i.id.clone())),
        };
        let Some(id) = id else {
            return;
        };
        let path = match panel {
            Panel::Clusters => format!("compute/clusters/{id}"),
            Panel::Jobs => format!("jobs/{id}"),
            Panel::Pipelines => format!("pipelines/{id}"),
            Panel::Warehouses => format!("sql/warehouses/{id}"),
            Panel::Dashboards => format!("sql/dashboardsv3/{id}"),
            Panel::Catalog => format!("explore/data/{}", id.replace('.', "/")),
            // Secret scopes have no workspace-UI page.
            Panel::Secrets => return,
        };
        let url = format!("{}/{}", host.trim_end_matches('/'), path);
        #[cfg(target_os = "macos")]
        let opener = "open";
        #[cfg(not(target_os = "macos"))]
        let opener = "xdg-open";
        let _ = std::process::Command::new(opener).arg(url).spawn();
    }

    /// Counts of (ok, pending, failed, idle) items across all panels.
    pub fn status_counts(&self) -> (usize, usize, usize, usize) {
        let (mut ok, mut pending, mut failed, mut idle) = (0, 0, 0, 0);
        for shape in self.shapes.iter().flatten() {
            if let Shape::List(items) = shape {
                for item in items {
                    match item.status {
                        Status::Running | Status::Success => ok += 1,
                        Status::Pending => pending += 1,
                        Status::Failed => failed += 1,
                        Status::Stopped => idle += 1,
                        Status::Unknown(_) => {}
                    }
                }
            }
        }
        (ok, pending, failed, idle)
    }

    pub fn last_refresh_age(&self) -> Duration {
        self.last_refresh.elapsed()
    }

    pub fn spinner(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()]
    }

    pub fn spinner_frame(&self) -> usize {
        self.spinner_frame
    }

    /// True whenever any background work is in flight — the loop uses this
    /// to keep spinners ticking, not just during panel refreshes.
    pub fn busy(&self) -> bool {
        self.loading
            || self.detail_rx.is_some()
            || self.action_rx.is_some()
            || self.preview_rx.is_some()
            || self.cost_rx.is_some()
            || self.sql_rx.is_some()
            || self.run_rx.is_some()
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
    }

    pub fn focus_next(&mut self) {
        self.cycle_focus(1);
    }

    pub fn focus_prev(&mut self) {
        self.cycle_focus(-1);
    }

    /// Cycles focus through the visible panes in display order.
    fn cycle_focus(&mut self, delta: i32) {
        let visible = self.visible_panes();
        if visible.is_empty() {
            return;
        }
        let focus_idx = Panel::ALL
            .iter()
            .position(|p| p == &self.focus)
            .unwrap_or(0);
        let pos = visible.iter().position(|&i| i == focus_idx).unwrap_or(0);
        let n = visible.len() as i32;
        let next = ((pos as i32 + delta) % n + n) % n;
        self.focus = Panel::ALL[visible[next as usize]];
    }

    pub fn needs_refresh(&self) -> bool {
        !self.loading && self.last_refresh.elapsed() >= self.refresh_interval
    }

    pub fn start_refresh(&mut self, cli: &Arc<DatabricksCli>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.last_refresh = Instant::now();

        let (tx, rx) = mpsc::unbounded_channel();
        self.pending = Some(rx);
        self.in_flight = 8;

        // One task per source so each panel updates as soon as its fetch lands,
        // instead of waiting for the slowest of the five.
        macro_rules! spawn_fetch {
            ($update:expr, $fetch:path) => {{
                let cli = Arc::clone(cli);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = $fetch(&cli).await.map_err(|e| format!("{e:#}"));
                    let _ = tx.send($update(result));
                });
            }};
        }

        spawn_fetch!(|s| Update::Panel(0, s), fetchers::clusters::fetch);
        spawn_fetch!(|s| Update::Panel(1, s), fetchers::jobs::fetch);
        spawn_fetch!(|s| Update::Panel(2, s), fetchers::pipelines::fetch);
        spawn_fetch!(|s| Update::Panel(3, s), fetchers::warehouses::fetch);
        spawn_fetch!(|s| Update::Panel(4, s), fetchers::dashboards::fetch);
        spawn_fetch!(
            |s: Result<Shape, String>| Update::Badge(s.ok()),
            fetchers::current_user::fetch
        );
        {
            let cli = Arc::clone(cli);
            let tx = tx.clone();
            let path = self.uc_path.clone();
            tokio::spawn(async move {
                let result = fetchers::catalog::fetch(&cli, &path)
                    .await
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Update::Panel(5, result));
            });
        }
        {
            let cli = Arc::clone(cli);
            let tx = tx.clone();
            let scope = self.secret_scope.clone();
            tokio::spawn(async move {
                let result = fetchers::secrets::fetch(&cli, scope.as_deref())
                    .await
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Update::Panel(6, result));
            });
        }
    }

    /// Applies any fetch results that have arrived; returns true if the UI should redraw.
    pub fn poll_refresh(&mut self) -> bool {
        let Some(rx) = &mut self.pending else {
            return false;
        };
        let mut changed = false;
        let mut updated_panes: Vec<usize> = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(Update::Panel(i, result)) => {
                    match result {
                        Ok(mut shape) => {
                            // Active work floats to the top of every pane
                            // except the catalog, which stays browsable
                            // in its natural (alphabetical) order.
                            if i != 5 {
                                if let Shape::List(items) = &mut shape {
                                    items.sort_by_key(|it| {
                                        (it.status.rank(), it.history.is_empty())
                                    });
                                }
                            }
                            self.shapes[i] = Some(shape);
                            self.updated_at[i] = Some(Instant::now());
                            updated_panes.push(i);
                        }
                        // Keep previous data on failure so panels don't blank
                        // out — but surface the error if there's nothing yet.
                        Err(e) => {
                            if matches!(self.shapes[i], None | Some(Shape::Text(_))) {
                                self.shapes[i] = Some(Shape::Text(format!("✗ {e}")));
                            }
                        }
                    }
                    self.in_flight -= 1;
                    changed = true;
                }
                Ok(Update::Badge(badge)) => {
                    if badge.is_some() {
                        self.user_badge = badge;
                    }
                    self.in_flight -= 1;
                    changed = true;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.in_flight = 0;
                    break;
                }
            }
        }
        for i in updated_panes {
            self.alert_new_failures(i);
        }
        if self.in_flight == 0 {
            self.loading = false;
            self.pending = None;
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::{from_table, token_at_cursor};

    #[test]
    fn token_bare_word() {
        let (start, ctx, prefix) = token_at_cursor("SELECT * FROM ma", 16);
        assert_eq!((start, ctx.as_str(), prefix.as_str()), (14, "", "ma"));
    }

    #[test]
    fn token_dotted_path() {
        let (start, ctx, prefix) = token_at_cursor("SELECT * FROM main.sales.or", 27);
        assert_eq!(
            (start, ctx.as_str(), prefix.as_str()),
            (25, "main.sales", "or")
        );
    }

    #[test]
    fn token_trailing_dot() {
        let (start, ctx, prefix) = token_at_cursor("main.", 5);
        assert_eq!((start, ctx.as_str(), prefix.as_str()), (5, "main", ""));
    }

    #[test]
    fn token_mid_input() {
        // Caret inside the statement, not at the end.
        let (start, ctx, prefix) = token_at_cursor("SELECT co FROM t", 9);
        assert_eq!((start, ctx.as_str(), prefix.as_str()), (7, "", "co"));
    }

    #[test]
    fn from_table_fully_qualified() {
        assert_eq!(
            from_table("SELECT x FROM main.sales.orders WHERE x > 1").as_deref(),
            Some("main.sales.orders")
        );
    }

    #[test]
    fn from_table_rejects_partial_names() {
        assert_eq!(from_table("SELECT x FROM orders"), None);
        assert_eq!(from_table("SELECT x FROM main.sales."), None);
        assert_eq!(from_table("SELECT 1"), None);
    }
}
