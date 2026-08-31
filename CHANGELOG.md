# Changelog

## [Unreleased]

### Added
- All 63 US National Parks as color themes, in dark and light, taking the
  total from 16 to 142. They come from
  [national-parks-themes](https://github.com/pjhamera/national-parks-themes)
  via `scripts/gen_parks_themes.py`, which ports that project's OKLCH math
  and then checks its own output against the terminal schemes the project
  publishes — 1386 values across all 126 variants, so these are upstream's
  palettes rather than an approximation of them, save for the body-text
  tint described below. No new runtime dependencies: the colours are
  committed as `src/theme/parks.rs`.
- `--list-themes` prints every theme id, grouped by origin and background,
  so `--help` no longer has to enumerate them.

### Changed
- `t` opens a searchable theme picker instead of stepping to the next
  theme — cycling was never going to be a way through 142 palettes. Type
  to filter by name, id, park, state or description ("california" and
  "granite" both find Yosemite); `↑`/`↓` preview live so you see a theme
  before choosing it, `Enter` keeps it, `Esc` puts back what you had.
- The picker lists dark and light separately — 76 dark themes or 66 light
  ones, never interleaved — and opens on whichever kind the current theme
  is, so on a light terminal you no longer scroll past palettes built for
  a dark one. `Tab` crosses over, keeping the query and following the
  `-light` twin where there is one, so `parks-zion` ⇄ `parks-zion-light`
  is a single keystroke.
- An unknown `theme` id in `config.json` flashes a warning at startup
  instead of silently falling back to dark, and an unknown `--theme` value
  now exits pointing at `--list-themes`.
- Themes are held in a data table rather than five parallel lists and a
  280-line match. Every existing theme id is unchanged, including the
  `gruvbox` / `gruvbox-light` spelling, so saved preferences keep working.
- Park themes tint their body text toward the park's own hue, so they are
  told apart by more than the accent swatches. Upstream authors `fg` for
  terminals that also paint `bg`, letting a park's character sit in the
  background; this TUI never paints one, so all 63 parks were arriving as
  the same near-neutral off-white — chroma 0.017 on average, which reads
  as grey. `gen_parks_themes.py` now keeps each park's authored hue and
  raises only its chroma, to an average of 0.041, in the range the
  built-ins already occupy (kanagawa 0.039, tokyo-night 0.061). Text is
  iced blue on Denali and sandstone on Zion; contrast is untouched, and
  `--verify` still diffs the pre-tint colours against upstream.

## [0.33.0] - 2026-08-23

### Added
- Health report: `i` on a job opens a deep per-job diagnostic view —
  success rate, duration trend and per-task failure attribution over the
  last 30 days from `system.lakeflow.job_run_timeline` and
  `job_task_run_timeline`, CPU/memory pressure from
  `system.compute.node_timeline` joined to the run's own clusters, and
  threshold-based flags (memory pressure, over-provisioning, I/O wait,
  node-type mismatch) built directly from that data. A best-effort probe
  of the job's most recent run adds per-stage spill and task-duration
  skew: the live driver first (fastest, while the cluster's up), falling
  back to reading the cluster's own delivered Spark event log once the
  driver reports the cluster terminated (needs `cluster_log_conf`
  configured with a DBFS destination). Scans back through a few recent
  runs rather than only the newest, since the newest is often exactly
  the kind with nothing to probe — skipped, disabled, a condition that
  wasn't met — and uses the first one that actually has a cluster.
  Degrades to a plain unavailable message on any failure without
  affecting the rest of the report. Once both signals are in, the FLAGS
  list cross-references them — spill that matches sustained memory
  pressure calls out a memory-optimized node type more confidently, and
  task skew is called out separately as a data/partitioning issue rather
  than something more compute would fix.

## [0.32.1] - 2026-08-10

### Fixed
- The record view had no scroll key within reach. `↑`/`↓` stayed bound to
  history, which no-ops unless you are already browsing it, so they did
  nothing at all once a row was transposed — leaving `PgUp`/`PgDn`, which
  step five fields at a time and which a Mac laptop keyboard only reaches
  via `Fn`+`↑`/`↓`. `↑`/`↓` now walk the fields one by one while the
  record view is up, the way `j`/`k` already did in preview's, and go
  back to history the moment it closes. The footer follows, reading
  `↑/↓ fields` and `pgup/pgdn page` instead of advertising history.

## [0.32.0] - 2026-08-10

### Added
- `F2` toggles the SQL console's record view, alongside `Ctrl+V`. Windows
  Terminal binds `Ctrl+V` to paste by default and consumes it before the
  app sees it, which left the view unreachable there — the keystroke
  pasted the clipboard into the prompt instead. Function keys are
  untouched by terminal defaults, and the console prompt is live so no
  printable character was available. The footer shows `f2` — the same two
  columns `^v` had, in a footer with none to spare — and `?` lists both.

### Fixed
- The event loop acts on key presses only. A Windows console reports a
  release for every press, so an unfiltered handler ran twice per
  keystroke: letters doubled, `j` jumped two rows, and toggles like
  record view turned on and straight back off within one keystroke.
  Windows Terminal and WSL never sent those releases, so this only bit a
  native binary in a bare conhost window. No-op on macOS and Linux.

## [0.31.0] - 2026-08-09

### Added
- Record view in the SQL console: `Ctrl+V` transposes the current result
  row into one field per line — the readable way through a result too
  wide to page across, matching what `v` already did in table previews.
  `Shift+←`/`→` step through rows, `PgUp`/`PgDn` walk the fields, and
  `Esc` peels back to the grid before closing the console. The prompt
  stays live throughout, so plain `←`/`→` still move the caret.

## [0.30.0] - 2026-08-09

### Added
- Per-item spend: `c` on a job or pipeline shows what that one thing cost
  over the last week, month, quarter and year, each against the window
  before it (▲ amber when spend is rising, ▼ when it's falling), plus a
  month-by-month bar chart of its year. Reads `system.billing.usage`
  through the same warehouse and workspace scoping as `$`, attributing
  usage via `usage_metadata.job_id` / `dlt_pipeline_id`. Jobs on
  all-purpose compute carry no job id in `usage`, so those say so
  instead of showing an empty chart.

## [0.29.0] - 2026-07-27

### Added
- Eight new color themes bring the total to sixteen: the Catppuccin family
  is now complete (Macchiato and Frappé join Mocha and Latte), plus Gruvbox
  Light, Rosé Pine, Everforest, Kanagawa, Solarized Dark and One Dark. `t`
  cycles through all of them and `--theme` accepts the new ids (e.g.
  `catppuccin-macchiato`, `rose-pine`, `kanagawa`, `one-dark`).

## [0.28.0] - 2026-07-27

### Added
- Favorites: `f` pins/unpins the selected item in any pane (marked with a
  `★`), and `F` toggles the focused pane to show only favorites. Pins are
  remembered per workspace profile in `~/.config/databricks-tui/config.json`.
  Drilling the Unity Catalog into a level with no pinned items shows
  everything, so favorites-only never dead-ends, and jumping to an item
  (`Ctrl+P` / problems) clears the filter so the target stays visible.

## [0.27.1] - 2026-07-20

### Fixed
- Sluggish and occasionally frozen terminal under fast input or busy
  refreshes: redraws are now coalesced and capped at ~60 fps instead of
  one full-screen draw per event, so a burst of key-repeat events no
  longer floods the terminal's write buffer. The spinner and splash
  animation run on an independent clock, and the loop still idles at
  ~0% CPU

## [0.27.0] - 2026-07-19

### Added
- Running-long detection: a live job run that has already taken 1.5×
  the median of its recent successful runs gets an amber `⚠ 2.5× usual`
  tag in the jobs pane, an entry in the problems view (`!`, local and
  cross-workspace) and a one-time bell + flash — hung runs no longer
  sit there looking green
- Trigger with parameters: `p` on the run confirm opens a prompt
  prefilled with the job's current parameter defaults (job-level
  `parameters`, or notebook `base_parameters` merged across tasks);
  edit the `key=value` pairs and Enter runs the job with the overrides
- Watch a run (`W` in a run view): the run is polled in the background
  and a terminal bell + flash fires the moment it finishes, success or
  failure; a `👁` counter in the header shows how many runs are being
  watched, and `W` again unwatches

## [0.26.0] - 2026-07-19

### Added
- Run-history grid (`g` in a run view): every task's state across the
  job's recent runs as an Airflow-style matrix, so a flaky task reads
  differently from a broken job at a glance; `h`/`l` moves the ▾ marker
  along the columns
- Task duration trends in the grid: each task row ends with a sparkline
  of its successful-run durations and a `▲1.6×` flag when the newest is
  at least 1.5× the median — creeping slowdowns become visible before
  they blow a deadline
- Pause/resume job schedules (`S` on the jobs pane): flips the pause
  status of the job's schedule, trigger or continuous mode in place —
  no confirm, pressing `S` again undoes it; the pane shows `⏸ paused`
  inline

## [0.25.0] - 2026-07-19

### Added
- Cross-workspace problems: `!` now also scans every other profile in
  ~/.databrickscfg in the background (clusters, jobs, pipelines,
  warehouses) and appends failures tagged "profile ▸ name"; Enter on a
  remote row switches to that workspace. Unreachable workspaces show as
  a single row instead of disappearing

## [0.24.0] - 2026-07-18

### Added
- Scrollbars: detail views, run views (summary, raw JSON, output,
  timeline, DAG), SQL results, table previews and the help overlay show
  a scrollbar on the right border when content overflows
- Colorized run output: section headers are tinted by task state, error
  lines (ERROR, exceptions, stack frames) red, WARN lines yellow, and
  leading log timestamps dimmed so the message carries the color
- SQL syntax highlighting in the console prompt (and history search):
  keywords bold, strings green, numbers yellow, quoted identifiers
  orange, comments dimmed — live as you type

### Changed
- Unfocused panes dim their text (names, details, table rows) while
  keeping status colors, so the focused pane stands out at a glance

## [0.23.0] - 2026-07-18

### Added
- Task DAG view: press `d` in a job run to see the tasks as a dependency
  tree — each task under the task it waits for, colored by state, with
  extra dependencies annotated
- Live output tailing: the `o` output view now keeps re-fetching while
  the run executes, so task output and errors stream in as tasks finish
  (the title shows "output (tailing)")
- Upcoming runs (`u`): every job with a cron schedule, periodic or
  file-arrival trigger, or continuous mode, sorted by next fire time
  with countdowns; Enter jumps to the job. The jobs pane shows the
  countdown inline ("1h ago · ⏱ in 27m") and job details gain a
  "Next run" row

## [0.22.0] - 2026-07-18

### Added
- Tab autocomplete in the SQL console: completes catalog, schema, table, and
  column names from Unity Catalog (fetched lazily, cached per session), plus
  common SQL keywords. Bare words after a fully-qualified `FROM` table complete
  against that table's columns. Tab cycles candidates, Esc restores what you
  typed, Enter accepts without running the statement.
- Run timeline view: press `t` in a job run to see each task's execution
  window as a Gantt-style bar on a shared time axis, colored by task state.
  Running tasks extend to now; the toggle sticks while paging runs with h/l.

## [0.21.0] - 2026-07-16

### Added
- Full run output view (`o` in a run): the complete error, stack trace, and
  log tail of every task via `jobs get-run-output`
- Repair run (`R`): re-run only the failed tasks of a job run
- Readable wide tables: preview and SQL results columns get sensible widths
  instead of clipping

### Changed
- Refreshed the demo GIF to cover the current feature set

## [0.20.4] - 2026-07-15

### Fixed
- Jobs with recent runs were shown as NO RUNS

## [0.20.3] - 2026-07-15

### Fixed
- Volume folders were shown as files and couldn't be entered

## [0.20.2] - 2026-07-15

### Fixed
- Enter on a secret key no longer errors

## [0.20.1] - 2026-07-15

### Fixed
- Secrets pane was empty against the real CLI (bare-array output)

## [0.20.0] - 2026-07-14

### Added
- Secret scopes pane
- Multi-hop lineage tree for Unity Catalog tables
- Permissions and troubleshooting guides in the docs

## [0.19.0] - 2026-07-14

### Added
- Pane arrangement mode (`H`): reorder and hide panes, persisted
- Help overlay (`?`) listing every shortcut

## [0.18.0] - 2026-07-14

### Added
- SQL alerts pane
- Cancel runs, pipeline updates, and queries in flight
- Command palette (`:` commands)
- File peek for volume files
- `DESCRIBE DETAIL` from the Unity Catalog pane

## [0.17.3] - 2026-07-14

### Changed
- Top spenders ranked by dollars when prices are available

## [0.17.2] - 2026-07-14

### Fixed
- crates.io publish failed on a dirty worktree; unchanged Homebrew formula
  no longer fails the release
- Flash messages no longer hide the footer shortcuts

## [0.17.1] - 2026-07-13

### Added
- Homebrew tap and crates.io publishing on release, with install docs

### Changed
- Release-prep polish

## [0.17.0] - 2026-07-13

### Added
- Persistent preferences (theme, layout, warehouse choice)
- External editor and search in the SQL console
- Volume browsing in the Unity Catalog pane

## [0.16.0] - 2026-07-13

### Added
- SQL statement history
- CSV export of query results
- Pipeline update drill-down

## [0.15.1] - 2026-07-13

### Fixed
- Full line editing (cursor movement, word ops) in the SQL console prompt

## [0.15.0] - 2026-07-13

### Changed
- Cost view scoped to the current workspace

## [0.14.0] - 2026-07-13

### Added
- Prefill the SQL console from the selected catalog table

## [0.13.0] - 2026-07-13

### Added
- Eight color themes

### Changed
- Unity Catalog listings sorted alphabetically

## [0.12.1] - 2026-07-13

### Fixed
- Problems overlay clipping with long names and notes

## [0.12.0] - 2026-07-13

### Added
- SQL console
- Job run drill-down
- Top spenders and problems views

## [0.11.1] - 2026-07-13

### Changed
- Gradient brand wordmark in the header

## [0.11.0] - 2026-07-13

### Added
- Active-first pane ordering
- `/` filter across panes

## [0.10.1] - 2026-07-13

### Changed
- Distinctive per-pane icons

## [0.10.0] - 2026-07-10

### Added
- Dollar estimates in the usage view
- Table lineage

## [0.9.0] - 2026-07-10

### Added
- Access views and warehouse query history
- DBU usage view from `system.billing.usage`

## [0.8.5] - 2026-07-10

### Added
- Warehouse type and a manual repro command in the preview diagnostic

### Fixed
- Neutralized bundle context so all CLI calls hit the same workspace

## [0.8.4] - 2026-07-10

### Added
- Diagnose preview warehouse failures

## [0.8.3] - 2026-07-10

### Fixed
- Warehouse picker polish and recovery from stale warehouse ids

## [0.8.2] - 2026-07-10

### Fixed
- Picker overlays were invisible while zoomed
- Tables/volumes listing failures surfaced in the Unity Catalog pane

## [0.8.1] - 2026-07-10

### Added
- Choose the SQL warehouse used for table previews

### Fixed
- Contextual footer hints; dropped the misrendering backspace glyph

## [0.8.0] - 2026-07-10

### Added
- Sample-data previews for Unity Catalog tables and views

## [0.7.0] - 2026-07-10

### Added
- Unity Catalog browser pane

## [0.6.0] - 2026-07-09

### Added
- Splash screen, status chips, and Databricks-branded visuals

## [0.5.0] - 2026-07-09

### Added
- Workspace switching
- Lakeview dashboards panel

## [0.4.3] - 2026-07-07

### Fixed
- Spinners keep ticking for all background work; fetch errors are surfaced

## [0.4.2] - 2026-07-07

### Fixed
- Workspace host resolved in the background to avoid a startup freeze

## [0.4.1] - 2026-07-07

### Added
- `--version` flag

## [0.4.0] - 2026-07-06

### Added
- Run insights, rich details, resource actions, and open-in-browser

## [0.3.3] - 2026-07-06

### Added
- Item selection and full-detail drill-down view

## [0.3.2] - 2026-07-06

### Added
- Light/dark theme toggle

### Changed
- Faster cluster loading

## [0.3.1] - 2026-07-06

### Added
- `uninstall` subcommand

## [0.3.0] - 2026-07-05

### Added
- Pane zoom
- Streaming refresh and `upgrade` command

### Changed
- Visual overhaul; fewer unnecessary redraws

## [0.2.0] - 2026-05-28

### Fixed
- Jobs and warehouses fetchers now handle plain array responses from the CLI
- `IDLE`, `DELETED` states now map to Stopped; `DELETING` maps to Pending
- Status labels show real text (e.g. `IDLE`) instead of `UNKNOWN`
- CI release job now has correct `contents: write` permission

### Changed
- Warehouses panel switched from table to list view with cluster size shown as detail
- All list items now render their detail field dimmed on the right

## [0.1.0] - 2026-05-28

### Added
- Initial scaffold: clusters, jobs, pipelines, warehouses panels
- Auto-refresh with configurable interval (`--refresh`)
- Multi-profile support (`--profile`)
- CI workflow with binary releases on git tags
