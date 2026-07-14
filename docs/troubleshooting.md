# Troubleshooting

## "The warehouse <id> was not found"

The warehouse id the app remembered doesn't exist in the workspace the
query was routed to. Two common causes:

- **Stale session memory** — the warehouse was deleted, or you switched
  workspaces. Press `P` (in the catalog pane) to pick a different
  warehouse; the choice is remembered per profile.
- **Wrong workspace routing** — older CLI versions resolve typed
  commands and raw `api` calls against *different* workspaces when run
  from inside a folder containing `databricks.yml` (a bundle). The app
  neutralizes this by running the CLI from `/`, but if you see split
  behavior in your own shell, that's why.

The error view shows a diagnostic distinguishing "warehouse exists but
you lack CAN USE" from "warehouse doesn't exist here".

## Query never finishes / warehouse stuck STARTING

Previews, the SQL console, cost and lineage all need the warehouse to
start, which can take minutes (or fail — check the warehouse health in
the workspace console). The app polls for ~3 minutes, then cancels the
statement so nothing stays queued. A running console statement can be
canceled earlier with `Esc`.

## Cost view says "all workspaces" or shows nothing

- "all workspaces" — the app couldn't resolve the current workspace's
  id from `system.access.workspaces_latest` (not readable, or the URL
  match wasn't unique), so it shows account-wide data rather than
  guessing.
- Empty or error — `system.billing.usage` isn't enabled or you can't
  read it. See [permissions](permissions.md).

## "tables unavailable" / "volumes unavailable" rows in the catalog

The listing call for that object kind failed — usually missing
`USE SCHEMA`/`SELECT` privileges, sometimes a transient API error. The
row shows the first line of the actual error.

## Auth errors on every pane

The CLI's token has expired or the profile is wrong:

```bash
databricks auth login              # refresh OAuth
databricks-tui --profile <name>    # or pick one with `w` in the app
```

## Icons look wrong / boxes instead of glyphs

The pane icons are plain Unicode (no Nerd Font needed), but a very old
terminal font may lack them. Any modern monospace font works; the app
avoids emoji-width characters so alignment never breaks.

## Where the app keeps files

Only `~/.config/databricks-tui/`: `history` (SQL console statements)
and `config.json` (theme, warehouse choice, pane layout). Both are
created owner-only (0600). Delete the directory to reset everything.
