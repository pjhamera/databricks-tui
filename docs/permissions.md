# What each feature needs

databricks-tui drives the official Databricks CLI with **your**
credentials — it can never see more than your user can. Most features
work with plain read access; the ones below have extra prerequisites.

| Feature | Key | Needs |
|---|---|---|
| Panes (compute, jobs, pipelines, warehouses, dashboards, catalog) | — | Whatever list/read access your user already has; items you can't see simply don't appear |
| Start/stop/run actions | `s` | Manage (clusters, warehouses, pipelines) or Can Manage Run (jobs) on the resource |
| Run drill-down with task errors | `Enter` in a job detail | Can View on the job; error output comes from `jobs get-run-output` |
| Pipeline update drill-down | `Enter` in a pipeline detail | Can View on the pipeline (update info + event log) |
| Cancel a run / stop an update | `s` in the run view | Can Manage Run (jobs) / Can Manage (pipelines) |
| Table previews & SQL console | `p`, `:` | **CAN USE** on at least one SQL warehouse |
| Table size/format facts (DESCRIBE DETAIL) | in table details | `SELECT` on the table + a usable warehouse |
| Access view | `g` | Ability to read grants on the object (owners and admins always can) |
| Volume browsing & file peek | `Enter` on a volume/file | `READ VOLUME` on the volume |
| Cost view | `$` | `SELECT` on `system.billing.usage`; dollar estimates also need `system.billing.list_prices` |
| Cost scoping to the current workspace | automatic | `SELECT` on `system.access.workspaces_latest` |
| Lineage | `L` | `SELECT` on `system.access.table_lineage` |

## About system tables

The cost and lineage features read [system tables](https://docs.databricks.com/aws/en/admin/system-tables/).
Two things must be true:

1. **The schemas are enabled** — an account admin enables `system.billing`
   and `system.access` once per metastore.
2. **You can read them** — a metastore admin grants access, e.g.:

   ```sql
   GRANT USE SCHEMA ON SCHEMA system.billing TO `your-group`;
   GRANT SELECT ON SCHEMA system.billing TO `your-group`;
   ```

The app degrades gracefully when something is missing:

- `list_prices` unreadable → the cost view shows DBUs without dollar
  estimates.
- `workspaces_latest` unreadable → the cost view shows the whole
  account, clearly labeled "all workspaces" with a warning line.
- `table_lineage` unreadable → the lineage view explains what it needs.

## Auth

All auth is the Databricks CLI's: profiles in `~/.databrickscfg`,
OAuth/PAT handled by the CLI itself. If `databricks clusters list`
works in your shell, the TUI works. The app stores no credentials.
