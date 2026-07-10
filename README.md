# databricks-tui

Terminal dashboard for Databricks — monitor compute, jobs, pipelines, SQL warehouses, dashboards, and Unity Catalog in one view.

- Animated splash screen and a Databricks-branded look: product-named panes
  (Compute, Lakeflow, SQL Warehouses, AI/BI Dashboards), status chips, and
  refresh flashes when new data lands
- Five color-coded panes that populate independently as each data source responds
- Jobs show their latest run result and a `✓✗✓` history strip, not just the config
- Health summary in the header: running / pending / failed / idle counts at a glance
- Drill into any item: formatted key facts and recent activity, raw JSON one key away
- Act on resources: start/stop clusters, warehouses and pipelines, trigger job runs
- Jump to any resource in the workspace web UI with one key
- Browse Lakeview dashboards: pages, widgets and datasets at a glance
- Unity Catalog browser: drill from catalogs into schemas, tables, views and
  volumes; table details include the full column schema, and `p` previews
  sample rows in a terminal table (runs SELECT … LIMIT 50 on a SQL warehouse)
- Switch between workspaces (CLI profiles) without restarting
- Zoom into any pane, light/dark theme, non-blocking refresh — the UI never freezes
- Built-in self-upgrade from GitHub releases

## Install

Download the latest release for your platform from the
[releases page](https://github.com/pjhamera/databricks-tui/releases):

```bash
# macOS (Apple Silicon)
curl -sL https://github.com/pjhamera/databricks-tui/releases/latest/download/databricks-tui-macos-arm64.tar.gz | tar xz
mv databricks-tui /usr/local/bin/
```

Artifacts: `databricks-tui-macos-arm64`, `databricks-tui-macos-x86_64`,
`databricks-tui-linux-x86_64` — each with a `.sha256` checksum.

Or build from source:

```bash
cargo install --path .
```

## Upgrade

```bash
databricks-tui upgrade
```

Detects your platform, checks the latest GitHub release, and replaces the
binary in place if a newer version exists.

## Uninstall

```bash
databricks-tui uninstall          # asks for confirmation
databricks-tui uninstall --yes    # no prompt
```

Removes the binary from wherever it is installed. The app keeps no other
files on your system.

## Usage

```bash
databricks-tui                      # default profile, 30s refresh
databricks-tui --profile prod       # named CLI profile
databricks-tui --refresh 10         # refresh every 10 seconds
databricks-tui --theme light        # light color scheme (default: dark)
```

The Clusters pane shows interactive (UI/API-created) clusters only —
job-created clusters are excluded, both for signal and because listing
them can be slow on busy workspaces.

## Keys

| Key | Action |
|-----|--------|
| `Tab` / `→` / `l` | Focus next panel |
| `Shift+Tab` / `←` / `h` | Focus previous panel |
| `↓` / `j`, `↑` / `k` | Select item in focused panel |
| `Enter` | Open details for the selected item (drills down in Unity Catalog) |
| `Backspace` | Go up one level in the Unity Catalog tree |
| `p` | Preview sample data for the selected table/view (may start a warehouse) |
| `s` | Action on selected item (start/stop, run job) — asks to confirm |
| `o` | Open selected item in the workspace web UI |
| `z` | Zoom focused panel to full screen |
| `w` | Switch workspace (pick a profile from ~/.databrickscfg) |
| `Esc` | Close details / exit zoom |
| `t` | Toggle light/dark theme |
| `r` | Force refresh |
| `q` / `Ctrl+C` | Quit |

Navigation works while zoomed — `Tab`/`h`/`l` jumps straight to the next
panel full-screen. In the details view, `j`/`k` scroll, `J` toggles the raw
JSON, `o` opens the browser, and `Esc` goes back.

## Requirements

- [Databricks CLI v0.200+](https://docs.databricks.com/dev-tools/cli/databricks-cli.html) installed and authenticated

## Release binaries

Push a `v*` tag to trigger a GitHub Actions build that publishes `.tar.gz`
binaries (with sha256 checksums and auto-generated release notes) for
Linux x86_64, macOS x86_64, and macOS ARM.
