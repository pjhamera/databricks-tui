use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use databricks_tui::{
    app::{App, ThemeMode},
    cli::{self as dbx, DatabricksCli},
    ui,
};
use ratatui::backend::CrosstermBackend;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "databricks-tui",
    about = "Terminal dashboard for Databricks",
    version
)]
struct Cli {
    #[arg(long, help = "Databricks CLI profile")]
    profile: Option<String>,

    #[arg(long, default_value = "30", help = "Auto-refresh interval in seconds")]
    refresh: u64,

    #[arg(long, value_enum, help = "Color theme (default: last used, then dark)")]
    theme: Option<ThemeArg>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ThemeArg {
    Dark,
    Light,
    CatppuccinMocha,
    CatppuccinLatte,
    Gruvbox,
    Dracula,
    Nord,
    TokyoNight,
}

impl From<ThemeArg> for ThemeMode {
    fn from(t: ThemeArg) -> Self {
        match t {
            ThemeArg::Dark => ThemeMode::Dark,
            ThemeArg::Light => ThemeMode::Light,
            ThemeArg::CatppuccinMocha => ThemeMode::CatppuccinMocha,
            ThemeArg::CatppuccinLatte => ThemeMode::CatppuccinLatte,
            ThemeArg::Gruvbox => ThemeMode::GruvboxDark,
            ThemeArg::Dracula => ThemeMode::Dracula,
            ThemeArg::Nord => ThemeMode::Nord,
            ThemeArg::TokyoNight => ThemeMode::TokyoNight,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Upgrade to the latest release from GitHub
    Upgrade,
    /// Remove the databricks-tui binary from your system
    Uninstall {
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    match args.command {
        Some(Command::Upgrade) => return tokio::task::spawn_blocking(upgrade).await?,
        Some(Command::Uninstall { yes }) => return uninstall(yes),
        None => {}
    }

    let cli = Arc::new(DatabricksCli::new(args.profile.clone()));
    let mut app = App::new(args.refresh, ThemeMode::Dark);
    // Flag beats remembered preference beats default.
    app.theme = args
        .theme
        .map(ThemeMode::from)
        .or_else(|| app.config.theme.as_deref().and_then(ThemeMode::from_id))
        .unwrap_or(ThemeMode::Dark);
    if args.theme.is_some() {
        app.persist_theme();
    }
    app.profiles = dbx::list_profiles();
    app.profile = args.profile.or_else(|| Some("DEFAULT".to_string()));
    app.restore_warehouse_pref();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = run(&mut terminal, &mut app, cli).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn upgrade() -> Result<()> {
    let target = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else {
        anyhow::bail!("no prebuilt binary for this platform — upgrade with `cargo install`");
    };

    let status = self_update::backends::github::Update::configure()
        .repo_owner("pjhamera")
        .repo_name("databricks-tui")
        .bin_name("databricks-tui")
        .target(target)
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()?
        .update()?;

    if status.updated() {
        println!("Upgraded to {}", status.version());
    } else {
        println!("Already up to date ({})", status.version());
    }
    Ok(())
}

fn uninstall(yes: bool) -> Result<()> {
    use std::io::Write;

    let exe = std::env::current_exe().context("could not locate the running binary")?;
    if !yes {
        print!("Remove {}? [y/N] ", exe.display());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !matches!(input.trim(), "y" | "Y" | "yes") {
            println!("Aborted.");
            return Ok(());
        }
    }
    std::fs::remove_file(&exe).with_context(|| format!("failed to remove {}", exe.display()))?;
    println!("Removed {}", exe.display());
    Ok(())
}

/// Suspends the TUI, opens $EDITOR on the current statement, and puts
/// the edited text back into the prompt.
fn edit_sql_in_editor(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let Some(current) = app.sql_input() else {
        return Ok(());
    };
    let path = std::env::temp_dir().join(format!("databricks-tui-{}.sql", std::process::id()));
    std::fs::write(&path, &current)?;
    databricks_tui::config::restrict(&path, 0o600);
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} '{}'", path.display()))
        .status();
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    if status.map(|s| s.success()).unwrap_or(false) {
        if let Ok(edited) = std::fs::read_to_string(&path) {
            app.sql_set_input(edited.trim());
        }
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

async fn run(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    mut cli: Arc<DatabricksCli>,
) -> Result<()> {
    terminal.draw(|f| ui::draw(f, app))?;
    let mut last_tick = Instant::now();

    // Workspace host for "open in browser", resolved in the background.
    app.fetch_host(&cli);

    loop {
        app.poll_host();
        let mut needs_redraw = app.poll_refresh();
        if app.poll_detail() {
            needs_redraw = true;
        }
        if app.poll_action(&cli) {
            needs_redraw = true;
        }
        if app.poll_uc() {
            needs_redraw = true;
        }
        if app.poll_secrets() {
            needs_redraw = true;
        }
        if app.poll_preview() {
            needs_redraw = true;
        }
        if app.poll_cost() {
            needs_redraw = true;
        }
        if app.poll_sql() {
            needs_redraw = true;
        }
        if app.poll_uc_names() {
            needs_redraw = true;
        }
        if app.poll_run(&cli) {
            needs_redraw = true;
        }
        if app.poll_upcoming() {
            needs_redraw = true;
        }

        // Splash: animate fast, expire on its deadline.
        if let Some(t) = app.splash_until {
            needs_redraw = true;
            if Instant::now() >= t {
                app.splash_until = None;
            }
        }

        // Redraw once a second while idle so the "updated Ns ago" counter stays live.
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            needs_redraw = true;
            if app.expire_flash() {
                needs_redraw = true;
            }
        }

        if app.needs_refresh() {
            app.start_refresh(&cli);
            needs_redraw = true;
        }

        // Poll faster while anything is loading so spinners animate smoothly.
        let timeout = if app.splash_active() {
            Duration::from_millis(70)
        } else if app.busy() || app.any_fresh() {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(250)
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if app.splash_active() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        _ => {
                            app.dismiss_splash();
                            needs_redraw = true;
                        }
                    }
                } else if app.confirm.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('y') | KeyCode::Char('Y'), _) => {
                            app.confirm_execute(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        _ => {
                            app.cancel_confirm();
                            needs_redraw = true;
                        }
                    }
                } else if app.picker.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        (KeyCode::Esc, _) | (KeyCode::Char('w'), _) => {
                            app.picker = None;
                            needs_redraw = true;
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.picker_next();
                            needs_redraw = true;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.picker_prev();
                            needs_redraw = true;
                        }
                        (KeyCode::Enter, _) => {
                            if let Some(new_cli) = app.picker_select() {
                                cli = new_cli;
                                app.start_refresh(&cli);
                                app.fetch_host(&cli);
                            }
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                } else if app.upcoming.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        (KeyCode::Esc, _) | (KeyCode::Char('u'), _) => {
                            app.close_upcoming();
                            needs_redraw = true;
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.upcoming_next();
                            needs_redraw = true;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.upcoming_prev();
                            needs_redraw = true;
                        }
                        (KeyCode::Enter, _) => {
                            app.upcoming_jump();
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                } else if app.problems.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        (KeyCode::Esc, _) | (KeyCode::Char('!'), _) => {
                            app.problems = None;
                            needs_redraw = true;
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.problems_next();
                            needs_redraw = true;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.problems_prev();
                            needs_redraw = true;
                        }
                        (KeyCode::Enter, _) => {
                            app.problems_jump();
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                } else if app.help {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.help_scroll = app.help_scroll.saturating_add(1)
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.help_scroll = app.help_scroll.saturating_sub(1)
                        }
                        _ => {
                            app.help = false;
                            app.help_scroll = 0;
                        }
                    }
                    needs_redraw = true;
                } else if app.pane_cfg.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Esc, _) | (KeyCode::Enter, _) | (KeyCode::Char('H'), _) => {
                            app.pane_cfg = None
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => app.pane_cfg_next(),
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => app.pane_cfg_prev(),
                        (KeyCode::Char(' '), _) => app.pane_cfg_toggle(),
                        (KeyCode::Char('J'), _) => app.pane_cfg_move(1),
                        (KeyCode::Char('K'), _) => app.pane_cfg_move(-1),
                        _ => {}
                    }
                    needs_redraw = true;
                } else if app.secret_form.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Esc, _) => app.secret_form = None,
                        (KeyCode::Enter, _) => app.secret_form_submit(&cli),
                        (KeyCode::Backspace, _) => app.secret_form_pop(),
                        (KeyCode::Char(ch), _) => app.secret_form_push(ch),
                        _ => {}
                    }
                    needs_redraw = true;
                } else if app.jump.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Esc, _) => app.jump = None,
                        (KeyCode::Enter, _) => app.jump_go(),
                        (KeyCode::Backspace, _) => app.jump_pop(),
                        (KeyCode::Down, _) => app.jump_next(),
                        (KeyCode::Up, _) => app.jump_prev(),
                        (KeyCode::Char('p'), KeyModifiers::CONTROL) => app.jump_next(),
                        (KeyCode::Char(ch), _) => app.jump_push(ch),
                        _ => {}
                    }
                    needs_redraw = true;
                } else if app.wh_picker.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        (KeyCode::Esc, _) => {
                            app.wh_picker_cancel();
                            needs_redraw = true;
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.wh_picker_next();
                            needs_redraw = true;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.wh_picker_prev();
                            needs_redraw = true;
                        }
                        (KeyCode::Enter, _) => {
                            app.wh_picker_select(&cli);
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                } else if app.sql.is_some() && app.sql_complete.is_some() {
                    // Completion popup: tab cycles, esc reverts, anything
                    // else keeps the insertion and falls back to typing.
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Tab, _) | (KeyCode::Down, _) => app.sql_complete_next(1),
                        (KeyCode::BackTab, _) | (KeyCode::Up, _) => app.sql_complete_next(-1),
                        (KeyCode::Esc, _) => app.sql_complete_cancel(),
                        (KeyCode::Enter, _) => app.sql_complete_accept(),
                        (KeyCode::Backspace, _) => {
                            app.sql_complete_accept();
                            app.sql_pop();
                        }
                        (KeyCode::Char(ch), _) => {
                            app.sql_complete_accept();
                            app.sql_push(ch);
                        }
                        _ => app.sql_complete_accept(),
                    }
                    needs_redraw = true;
                } else if app.sql.is_some() && app.hist_search.is_some() {
                    // Ctrl+R incremental search over past statements.
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Char('r'), KeyModifiers::CONTROL) => app.hist_search_older(),
                        (KeyCode::Esc, _) => app.hist_search_cancel(),
                        (KeyCode::Enter, _) => app.hist_search_accept(),
                        (KeyCode::Backspace, _) => app.hist_search_pop(),
                        (KeyCode::Char(ch), _) => app.hist_search_push(ch),
                        _ => app.hist_search_cancel(),
                    }
                    needs_redraw = true;
                } else if app.sql.is_some() {
                    // Printable keys type into the prompt.
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Char('a'), KeyModifiers::CONTROL) => app.sql_home(),
                        (KeyCode::Char('e'), KeyModifiers::CONTROL) => app.sql_end(),
                        (KeyCode::Char('s'), KeyModifiers::CONTROL) => app.sql_export(),
                        (KeyCode::Char('r'), KeyModifiers::CONTROL) => app.hist_search_start(),
                        (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                            edit_sql_in_editor(terminal, app)?;
                        }
                        // Esc cancels a running statement server-side;
                        // when idle it closes the console.
                        (KeyCode::Esc, _) if app.sql.as_ref().is_some_and(|c| c.running) => {
                            app.sql_cancel(&cli)
                        }
                        (KeyCode::Esc, _) => app.close_sql(),
                        (KeyCode::Enter, _) => app.sql_run(&cli),
                        (KeyCode::Tab, _) => app.sql_tab(&cli),
                        (KeyCode::Backspace, _) => app.sql_pop(),
                        (KeyCode::Delete, _) => app.sql_delete(),
                        (KeyCode::Left, KeyModifiers::SHIFT) => app.sql_cols(-1),
                        (KeyCode::Right, KeyModifiers::SHIFT) => app.sql_cols(1),
                        (KeyCode::Left, _) => app.sql_left(),
                        (KeyCode::Right, _) => app.sql_right(),
                        (KeyCode::Home, _) => app.sql_home(),
                        (KeyCode::End, _) => app.sql_end(),
                        (KeyCode::Up, _) => app.sql_hist_prev(),
                        (KeyCode::Down, _) => app.sql_hist_next(),
                        (KeyCode::PageUp, _) => app.sql_scroll(-5),
                        (KeyCode::PageDown, _) => app.sql_scroll(5),
                        (KeyCode::Char(ch), _) => app.sql_push(ch),
                        _ => {}
                    }
                    needs_redraw = true;
                } else if app.cost.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        (KeyCode::Esc, _) => {
                            app.close_cost();
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                } else if app.preview.is_some() {
                    let pv_entry = app.preview.as_ref().is_some_and(|pv| pv.filter_entry);
                    let pv_record = app.preview.as_ref().is_some_and(|pv| pv.record);
                    let pv_filtered = app.preview.as_ref().is_some_and(|pv| !pv.filter.is_empty());
                    if pv_entry {
                        // Column filter: printable keys type into the query.
                        match (key.code, key.modifiers) {
                            (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                            (KeyCode::Esc, _) => app.preview_filter_clear(),
                            (KeyCode::Enter, _) => app.preview_filter_accept(),
                            (KeyCode::Backspace, _) => app.preview_filter_pop(),
                            (KeyCode::Char(ch), _) => app.preview_filter_push(ch),
                            _ => {}
                        }
                        needs_redraw = true;
                    } else {
                        match (key.code, key.modifiers) {
                            (KeyCode::Char('q'), _)
                            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                            // Esc peels back one layer: record view, then
                            // the column filter, then the preview itself.
                            (KeyCode::Esc, _) if pv_record => app.preview_toggle_record(),
                            (KeyCode::Esc, _) if pv_filtered => app.preview_filter_clear(),
                            (KeyCode::Esc, _) => app.close_preview(),
                            (KeyCode::Char('/'), _) => app.preview_filter_start(),
                            (KeyCode::Char('v'), _) | (KeyCode::Enter, _) => {
                                app.preview_toggle_record()
                            }
                            (KeyCode::Char('e'), _) => app.preview_export(),
                            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => app.preview_scroll(1),
                            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => app.preview_scroll(-1),
                            (KeyCode::Left, _) | (KeyCode::Char('h'), _) => app.preview_h(-1),
                            (KeyCode::Right, _) | (KeyCode::Char('l'), _) => app.preview_h(1),
                            _ => {}
                        }
                        needs_redraw = true;
                    }
                } else if app.run_view.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        // Esc closes the output view first, then the
                        // timeline, then the run.
                        (KeyCode::Esc, _)
                            if app.run_view.as_ref().is_some_and(|rv| rv.show_output) =>
                        {
                            app.run_toggle_output(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Esc, _)
                            if app.run_view.as_ref().is_some_and(|rv| rv.show_timeline) =>
                        {
                            app.run_toggle_timeline();
                            needs_redraw = true;
                        }
                        (KeyCode::Esc, _)
                            if app.run_view.as_ref().is_some_and(|rv| rv.show_dag) =>
                        {
                            app.run_toggle_dag();
                            needs_redraw = true;
                        }
                        (KeyCode::Esc, _) => {
                            app.close_run();
                            needs_redraw = true;
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.run_scroll(1);
                            needs_redraw = true;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.run_scroll(-1);
                            needs_redraw = true;
                        }
                        (KeyCode::Left, _) | (KeyCode::Char('h'), _) => {
                            app.run_nav(&cli, 1);
                            needs_redraw = true;
                        }
                        (KeyCode::Right, _) | (KeyCode::Char('l'), _) => {
                            app.run_nav(&cli, -1);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('J'), _) => {
                            app.run_toggle_raw();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('o'), _) => {
                            app.run_toggle_output(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('t'), _) => {
                            app.run_toggle_timeline();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('d'), _) => {
                            app.run_toggle_dag();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('r'), _) => {
                            app.request_run_repair();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('s'), _) => {
                            app.request_run_cancel();
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                } else if app.detail.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        (KeyCode::Esc, _) => {
                            app.close_detail();
                            needs_redraw = true;
                        }
                        (KeyCode::Enter, _) => {
                            app.open_run(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.detail_scroll(1);
                            needs_redraw = true;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.detail_scroll(-1);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('J'), _) => {
                            app.toggle_raw();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('o'), _) => {
                            app.open_in_browser();
                        }
                        _ => {}
                    }
                } else if app.filter_entry {
                    // All printable keys go into the query while typing.
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Esc, _) => app.filter_clear(),
                        (KeyCode::Enter, _) => app.filter_accept(),
                        (KeyCode::Backspace, _) => app.filter_pop(),
                        (KeyCode::Char(ch), _) => app.filter_push(ch),
                        _ => {}
                    }
                    needs_redraw = true;
                } else {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
                        }
                        (KeyCode::Char('/'), _) => {
                            app.filter_start();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('!'), _) => {
                            app.open_problems();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('u'), _) => {
                            app.open_upcoming(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                            app.open_jump();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('H'), _) => {
                            app.open_pane_cfg();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('a'), _) => {
                            app.open_secret_form();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('x'), _) => {
                            app.request_secret_delete();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('?'), _) => {
                            app.help = true;
                            app.help_scroll = 0;
                            needs_redraw = true;
                        }
                        (KeyCode::Char(':'), _) => {
                            app.open_sql();
                            needs_redraw = true;
                        }
                        (KeyCode::Esc, _) if !app.active_filter().is_empty() => {
                            app.filter_clear();
                            needs_redraw = true;
                        }
                        (KeyCode::Tab, _) | (KeyCode::Right, _) | (KeyCode::Char('l'), _) => {
                            app.focus_next();
                            needs_redraw = true;
                        }
                        (KeyCode::BackTab, _) | (KeyCode::Left, _) | (KeyCode::Char('h'), _) => {
                            app.focus_prev();
                            needs_redraw = true;
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            app.select_next();
                            needs_redraw = true;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            app.select_prev();
                            needs_redraw = true;
                        }
                        (KeyCode::Enter, _) => {
                            if !app.secrets_drill(&cli) && !app.uc_drill(&cli) {
                                app.open_detail(&cli);
                            }
                            needs_redraw = true;
                        }
                        (KeyCode::Backspace, _) if app.secrets_up(&cli) || app.uc_up(&cli) => {
                            needs_redraw = true;
                        }
                        (KeyCode::Char('s'), _) => {
                            app.request_action();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('o'), _) => {
                            app.open_in_browser();
                        }
                        (KeyCode::Char('p'), _) => {
                            app.open_preview(&cli, false);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('g'), _) => {
                            app.open_grants(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('$'), _) => {
                            app.open_cost(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('L'), _) => {
                            app.open_lineage(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('P'), _) => {
                            app.open_preview(&cli, true);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('r'), _) => {
                            app.start_refresh(&cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('t'), _) => {
                            app.theme = app.theme.toggled();
                            app.persist_theme();
                            app.flash =
                                Some((format!("✓ theme: {}", app.theme.name()), Instant::now()));
                            needs_redraw = true;
                        }
                        (KeyCode::Char('w'), _) => {
                            app.open_picker();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('z'), _) => {
                            app.toggle_zoom();
                            needs_redraw = true;
                        }
                        (KeyCode::Esc, _) if app.zoomed => {
                            app.zoomed = false;
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.busy() || app.splash_active() {
            app.tick_spinner();
            needs_redraw = true;
        }
        if app.any_fresh() {
            needs_redraw = true;
        }

        if needs_redraw {
            terminal.draw(|f| ui::draw(f, app))?;
        }
    }
    Ok(())
}
