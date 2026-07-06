use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use databricks_tui::{
    app::{App, ThemeMode},
    cli::DatabricksCli,
    ui,
};
use ratatui::backend::CrosstermBackend;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "databricks-tui", about = "Terminal dashboard for Databricks")]
struct Cli {
    #[arg(long, help = "Databricks CLI profile")]
    profile: Option<String>,

    #[arg(long, default_value = "30", help = "Auto-refresh interval in seconds")]
    refresh: u64,

    #[arg(long, value_enum, default_value_t = ThemeArg::Dark, help = "Color theme")]
    theme: ThemeArg,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ThemeArg {
    Dark,
    Light,
}

impl From<ThemeArg> for ThemeMode {
    fn from(t: ThemeArg) -> Self {
        match t {
            ThemeArg::Dark => ThemeMode::Dark,
            ThemeArg::Light => ThemeMode::Light,
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

    let cli = Arc::new(DatabricksCli::new(args.profile));
    let mut app = App::new(args.refresh, args.theme.into());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = run(&mut terminal, &mut app, &cli).await;

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

async fn run(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    cli: &Arc<DatabricksCli>,
) -> Result<()> {
    terminal.draw(|f| ui::draw(f, app))?;
    let mut last_tick = Instant::now();

    // Workspace host for "open in browser"; auth describe works even offline.
    if let Ok(json) = cli.run(&["auth", "describe"]).await {
        app.host = json["details"]["host"]
            .as_str()
            .or_else(|| json["host"].as_str())
            .map(str::to_string);
    }

    loop {
        let mut needs_redraw = app.poll_refresh();
        if app.poll_detail() {
            needs_redraw = true;
        }
        if app.poll_action(cli) {
            needs_redraw = true;
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
            app.start_refresh(cli);
            needs_redraw = true;
        }

        // Poll faster while loading so the spinner animates smoothly.
        let timeout = if app.loading {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(250)
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if app.confirm.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('y') | KeyCode::Char('Y'), _) => {
                            app.confirm_execute(cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        _ => {
                            app.cancel_confirm();
                            needs_redraw = true;
                        }
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
                } else {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            break
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
                            app.open_detail(cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('s'), _) => {
                            app.request_action();
                            needs_redraw = true;
                        }
                        (KeyCode::Char('o'), _) => {
                            app.open_in_browser();
                        }
                        (KeyCode::Char('r'), _) => {
                            app.start_refresh(cli);
                            needs_redraw = true;
                        }
                        (KeyCode::Char('t'), _) => {
                            app.theme = app.theme.toggled();
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

        if app.loading {
            app.tick_spinner();
            needs_redraw = true;
        }

        if needs_redraw {
            terminal.draw(|f| ui::draw(f, app))?;
        }
    }
    Ok(())
}
