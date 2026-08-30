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
| Full task output and logs | `o` in the run view | Can View on the job; one `jobs get-run-output` call per task |
| Repair a failed run | `r` in the run view | Can Manage Run on the job; uses `jobs repair-run` |
| Table previews & SQL console | `p`, `:` | **CAN USE** on at least one SQL warehouse |
| Table size/format facts (DESCRIBE DETAIL) | in table details | `SELECT` on the table + a usable warehouse |
| Access view | `g` | Ability to read grants on the object (owners and admins always can) |
| Volume browsing & file peek | `Enter` on a volume/file | `READ VOLUME` on the volume |
| Cost view | `$` | `SELECT` on `system.billing.usage`; dollar estimates also need `system.billing.list_prices` |
| Per-job/pipeline spend | `c` | Same as the cost view |
| Cost scoping to the current workspace | automatic | `SELECT` on `system.access.workspaces_latest` |
| Lineage | `L` | `SELECT` on `system.access.table_lineage` |
| Job health report | `i` | `SELECT` on `system.lakeflow.job_run_timeline` and `job_task_run_timeline` (the latter also feeds compute pressure via `system.compute.node_timeline`/`clusters`); both degrade gracefully if unreadable. Spark diagnostics (skew/spill) need no extra grant against the run's live driver; falling back to the delivered event log after the cluster terminates additionally needs `READ` on the cluster's `cluster_log_conf` DBFS destination |
| AI job doctor | `d` in the health report | Opt-in and off by default: needs `doctor_endpoint` set in `~/.config/databricks-tui/config.json`, `CAN QUERY` on that model serving endpoint, and `CAN USE` on the warehouse the health report is already using (the call goes through `ai_query` there). Nothing extra is read — the digest is built from the health report you can already see |

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
- `job_task_run_timeline` unreadable → the health report shows run/duration
  trends without the per-task failure breakdown, and without CPU/memory
  pressure or the node-type heuristic (those are also sourced from this
  table's `compute_ids`, not the job-level table's).
- `system.compute` unreadable → same as above: CPU/memory pressure and
  the node-type heuristic are skipped, run/duration trends still show.

Spark diagnostics (skew/spill) in the health report try two sources for
the job's most recent run, in order:

1. **The live driver**, via an **undocumented** Databricks endpoint (the
   driver-proxy path in front of a run's Spark UI). Fast, and the
   freshest data while the cluster is still up — but confirmed to fail
   with a clean error the moment the cluster terminates, not to keep
   serving cached results.
2. **The delivered Spark event log**, read directly as a file — this is
   Apache Spark's own documented, open event-log format, not a
   Databricks-private one — from wherever the cluster's `cluster_log_conf`
   points (DBFS destinations only; S3/ADLS/GCS destinations aren't
   readable without separate cloud credentials the app doesn't have).
   Only tried once the live driver reports the cluster as terminated.
   Needs cluster log delivery to have been configured on the cluster (or
   its policy) in the first place — if it wasn't, there's nothing to
   read. If the event log has already rolled over past its most recent
   segment (large/long-running jobs), the rolled `.gz` segments aren't
   read.

Serverless runs have no driver or delivered logs to reach at all. Any
failure at any step — cluster gone, no delivery configured, no data,
timeout — shows a plain "unavailable" note explaining which step failed,
rather than an error, and never affects the rest of the health report.

Attribution has limits of its own: `usage` only carries a `job_id` for
runs on job or serverless compute, so a job running on all-purpose
compute shows no per-job spend — its DBUs belong to the shared cluster.
Pipelines are attributed through `dlt_pipeline_id` and don't have this
gap. Per-item spend also only reaches back 365 days, the retention of
`system.billing.usage`, which is why the year has nothing to compare
itself against.

## About the AI doctor

The doctor (`d` in the health report) is the only feature in the app that
can bill. It is off until you point it at a model serving endpoint:

```json
{ "doctor_endpoint": "databricks-meta-llama-3-3-70b-instruct" }
```

in `~/.config/databricks-tui/config.json`. With no endpoint set, the code
path never runs.

Even configured, four things keep it cheap:

- **It only fires on a keypress.** Nothing on a refresh timer calls it,
  and the health report itself is entirely free.
- **The rules gate the call.** A job whose thresholds all passed, or
  whose failures have no retrievable error text, is refused with the
  reason shown in place — a healthy job never costs anything. A job with
  fewer than 5 runs in the window is refused too, unless there's error
  text to read, since a statistical complaint off three runs is noise.
- **Verdicts are cached on the evidence.** Identical evidence reuses the
  previous answer from `~/.config/databricks-tui/doctor-cache.json` (64
  entries, oldest evicted), so you pay once per real change in the job's
  condition, not once per redraw. The pane says `cached — this diagnosis
  cost nothing` when it did.
- **Both ends are bounded.** ~6k characters of digest go in, 400 tokens
  come back. The pane prints an estimated token count next to the answer.

What gets sent is the digest, not the data: roughly twenty derived
numbers already visible in the report, the flags, and one truncated
stack trace with anything credential-shaped blanked. Raw run rows and
event logs never leave your machine — and the call itself goes through
`ai_query` on your own warehouse to your own endpoint, so the error text
stays inside your workspace.

Each prescription is rendered with the line of evidence it rests on. If
a prescription can't cite anything, that's the signal it was invented.

## Auth

All auth is the Databricks CLI's: profiles in `~/.databrickscfg`,
OAuth/PAT handled by the CLI itself. If `databricks clusters list`
works in your shell, the TUI works. The app stores no credentials.
