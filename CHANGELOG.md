# Changelog

## [Unreleased]

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
