//! Opt-in AI diagnosis layer over `job_health`.
//!
//! The rule-based flags in `job_health` already diagnose and prescribe
//! for everything a threshold can see — success rate, slowdown, memory
//! pressure, CPU wait, task attribution, spill and skew. This module
//! exists for the one signal a threshold *can't* read: the free text of
//! a failed task's error and stack trace, where an OOM, a permission
//! denial, an upstream schema change and a transient 429 all look
//! identical to a rule and call for completely different fixes.
//!
//! It is the only feature in this app that spends money, so the whole
//! module is built around not spending it:
//!
//! - **Off unless configured.** `Config::doctor_endpoint` is the flag
//!   and the endpoint in one field; `None` means the code below never
//!   runs.
//! - **Rules gate the call.** [`gate`] refuses to ask when the existing
//!   flags found nothing, when there's no error text, or when the run
//!   sample is too thin to diagnose. A healthy job never costs a cent.
//! - **Cached on the digest.** Identical evidence reuses the previous
//!   verdict from disk, so an auto-refreshing TUI pays once per real
//!   change in the job's condition rather than once per redraw.
//! - **The digest, never the data.** `JobHealthData` is already a
//!   compressed diagnosis; ~20 numbers, the flags and one truncated
//!   stack trace go over the wire, never raw run rows or event logs.
//! - **Bounded both ways.** [`MAX_DIGEST_CHARS`] caps input,
//!   [`MAX_OUTPUT_TOKENS`] caps output, and [`Usage`] reports what it
//!   cost so the meter can be shown next to the answer.
//!
//! The call goes through `ai_query` on the warehouse the health report
//! already opened — no new infra, no new auth, and the job's error text
//! never leaves the workspace, which is what lets the README keep
//! saying nothing is sent anywhere else.

use crate::cli::DatabricksCli;
use crate::config;
use crate::fetchers::job_health::{FlagSeverity, JobHealthData, NodeFamily};
use crate::fetchers::preview::run_sql;
use crate::fetchers::{jobs, runs, spark_live};
use crate::shape::Status;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Hard ceiling on the digest. ~6k chars is roughly 1.5k tokens — enough
/// for every derived signal plus a real stack trace, small enough that a
/// prescription costs a fraction of a cent on any endpoint worth using.
pub const MAX_DIGEST_CHARS: usize = 6_000;

/// Cap on the model's reply. Three prescriptions with rationale fit
/// comfortably; anything longer is padding nobody reads in a TUI pane.
pub const MAX_OUTPUT_TOKENS: u32 = 400;

/// Below this many runs in the window, trend and rate signals are noise.
/// Mirrors `job_health::MIN_RUN_SAMPLE`, which is private to that module.
const MIN_RUN_SAMPLE: u32 = 5;

/// How many failing tasks and how many diagnostic stages make it into
/// the digest. The tail is long and repetitive; the head is the story.
const MAX_TASKS_IN_DIGEST: usize = 3;
const MAX_STAGES_IN_DIGEST: usize = 3;

/// Verdicts kept on disk before the oldest are evicted.
const MAX_CACHE_ENTRIES: usize = 64;

/// Rough chars-per-token, for the cost meter only — never for a billing
/// claim. Good enough to tell "fractions of a cent" from "stop doing
/// this on a timer".
const CHARS_PER_TOKEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn label(&self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }

    fn parse(s: &str) -> Confidence {
        match s.trim().to_lowercase().as_str() {
            "high" => Confidence::High,
            "low" => Confidence::Low,
            _ => Confidence::Medium,
        }
    }
}

/// One ranked fix. `evidence` is the signal it rests on, quoted back
/// from the digest — a prescription that can't name its evidence is one
/// the model invented, and rendering the citation next to the advice is
/// what makes that visible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prescription {
    pub action: String,
    pub rationale: String,
    pub evidence: String,
    pub confidence: Confidence,
}

/// What the call cost, for the footer meter. Estimated from character
/// counts — the SQL statement API doesn't hand back token usage.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens_est: u32,
    pub completion_tokens_est: u32,
    /// True when this verdict came from disk and cost nothing this time.
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorVerdict {
    pub summary: String,
    pub prescriptions: Vec<Prescription>,
    pub usage: Usage,
}

/// The evidence bundle that would be sent, plus the hash that keys it in
/// the cache. Built before deciding to call so the same bytes that were
/// hashed are the bytes that get sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Digest {
    pub text: String,
    pub hash: u64,
}

impl Digest {
    pub fn key(&self) -> String {
        format!("{:016x}", self.hash)
    }
}

/// Outcome of the frugality check. Only [`Gate::Ask`] costs anything,
/// and the reason strings on the other two are meant to be rendered —
/// "why is there no AI answer here" is a question worth answering in
/// place rather than leaving the pane blank.
#[derive(Debug, Clone, PartialEq)]
pub enum Gate {
    /// Not worth a call. Carries the reason to show instead.
    Skip(String),
    /// Answered from disk; free.
    Cached(Box<DoctorVerdict>),
    /// Worth asking. Carries exactly what would be sent.
    Ask(Box<Digest>),
}

/// FNV-1a. Hand-rolled rather than `DefaultHasher` because the cache
/// outlives the process and `DefaultHasher`'s output is explicitly not
/// guaranteed stable across Rust releases — a silent rehash would turn
/// every cached verdict into a fresh charge after a toolchain bump.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Blanks anything in log text that looks like a credential. The call
/// stays inside the workspace, so this is belt-and-braces rather than a
/// boundary — but stack traces and log tails are exactly where a token
/// ends up pasted into a connection string, and a redacted digest is
/// also a digest that's safe to show the user before it's sent.
fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_bearer = false;
    for word in text.split_inclusive(char::is_whitespace) {
        let trimmed = word.trim_end();
        let trailing = &word[trimmed.len()..];
        if trimmed.is_empty() {
            // Runs of whitespace carry no word — pass them through
            // without clearing a pending `Bearer`.
            out.push_str(trailing);
            continue;
        }
        if after_bearer {
            out.push_str("***");
        } else {
            out.push_str(&redact_word(trimmed));
        }
        after_bearer = trimmed.eq_ignore_ascii_case("bearer");
        out.push_str(trailing);
    }
    out
}

/// A credential is usually the value half of a `token=dapi…` or
/// `Authorization:dapi…` pair rather than a bare word, so the shape test
/// is applied after the last `=` or `:` — and only the value is blanked,
/// leaving the key readable in the trace.
fn redact_word(word: &str) -> String {
    let cut = word.rfind(['=', ':']).map(|i| i + 1).unwrap_or(0);
    let (head, value) = word.split_at(cut);
    if looks_like_credential(value) {
        format!("{head}***")
    } else {
        word.to_string()
    }
}

/// Length is what separates a Databricks PAT from a log line that
/// happens to start with "dapi" — mangling ordinary text would cost the
/// evidence this call exists to read.
fn looks_like_credential(value: &str) -> bool {
    (value.starts_with("dapi") && value.len() > 20)
        || (value.starts_with("AKIA") && value.len() >= 20)
}

fn severity_label(s: FlagSeverity) -> &'static str {
    match s {
        FlagSeverity::Critical => "critical",
        FlagSeverity::Warn => "warn",
        FlagSeverity::Info => "info",
    }
}

fn family_label(f: NodeFamily) -> &'static str {
    match f {
        NodeFamily::Memory => "memory-optimized",
        NodeFamily::Compute => "compute-optimized",
        NodeFamily::General => "general-purpose",
        NodeFamily::Unknown => "unknown",
    }
}

/// True when the flags found something a human would want explained.
/// Info-level flags don't qualify on their own — "cluster looks
/// over-provisioned" is already the whole prescription.
fn has_actionable_flag(data: &JobHealthData) -> bool {
    data.flags
        .iter()
        .any(|f| matches!(f.severity, FlagSeverity::Warn | FlagSeverity::Critical))
}

fn has_stage_signal(live: Option<&spark_live::SparkLiveData>) -> bool {
    live.is_some_and(|l| {
        l.stages
            .iter()
            .any(|s| s.memory_bytes_spilled + s.disk_bytes_spilled > 0 || s.skew_ratio >= 3.0)
    })
}

/// The frugality check, in the order that spends least: config off →
/// nothing wrong → too little history → already answered → ask.
///
/// `failure_output` is the raw error text of the most recent failed run,
/// from [`latest_failure_output`]; `None` when nothing failed or the
/// fetch didn't land. It is the signal that most justifies a call, so a
/// job with error text gets past the thin-sample guard that a purely
/// statistical complaint would not.
pub fn gate(
    endpoint: Option<&str>,
    job_name: &str,
    data: &JobHealthData,
    live: Option<&spark_live::SparkLiveData>,
    failure_output: Option<&str>,
) -> Gate {
    if endpoint.is_none_or(str::is_empty) {
        return Gate::Skip(
            "doctor is off — set `doctor_endpoint` in ~/.config/databricks-tui/config.json to a \
             model serving endpoint to enable it"
                .to_string(),
        );
    }

    let failure_output = failure_output.filter(|s| !s.trim().is_empty());

    if !has_actionable_flag(data) && failure_output.is_none() && !has_stage_signal(live) {
        // A job can have failures that breached no threshold and whose
        // error output is no longer retrievable — an old failure that
        // the run history has scrolled past. Saying "nothing to
        // diagnose" next to a health view showing a red bar reads as a
        // bug even when the refusal is correct, so the two cases get
        // different sentences.
        return Gate::Skip(if data.failed_runs > 0 {
            format!(
                "{} of {} runs failed over {} days, but no threshold was breached and their \
                 error output is no longer retrievable — there's nothing to read beyond the \
                 numbers above",
                data.failed_runs, data.total_runs, data.window_days
            )
        } else {
            format!(
                "nothing to diagnose — all {} runs over {} days succeeded and no flag rose above \
                 info",
                data.total_runs, data.window_days
            )
        });
    }

    // A statistical complaint off three runs is a coin flip dressed up as
    // a finding; error text is direct evidence and stands on its own.
    if data.total_runs < MIN_RUN_SAMPLE && failure_output.is_none() {
        return Gate::Skip(format!(
            "only {} runs in the last {} days — too little history to diagnose",
            data.total_runs, data.window_days
        ));
    }

    let digest = build_digest(job_name, data, live, failure_output);
    match load_cached(&digest.key()) {
        Some(mut verdict) => {
            verdict.usage.cached = true;
            Gate::Cached(Box::new(verdict))
        }
        None => Gate::Ask(Box::new(digest)),
    }
}

/// Renders every derived signal as compact `key: value` lines, then
/// spends whatever's left of the character budget on error text.
///
/// Field order is fixed and every number is pre-rounded so that an
/// unchanged job produces byte-identical output — the cache depends on
/// that, and a digest that jittered in its last decimal place would miss
/// on every refresh and quietly bill for it.
pub fn build_digest(
    job_name: &str,
    data: &JobHealthData,
    live: Option<&spark_live::SparkLiveData>,
    failure_output: Option<&str>,
) -> Digest {
    let mut t = String::new();

    t.push_str(&format!("job: {job_name}\n"));
    t.push_str(&format!("window_days: {}\n", data.window_days));
    t.push_str(&format!(
        "runs: {} total, {} failed, {:.1}% success\n",
        data.total_runs, data.failed_runs, data.success_rate
    ));
    match data.duration_trend_pct {
        Some(p) => t.push_str(&format!(
            "duration_trend: {p:+.0}% recent third vs older third\n"
        )),
        None => t.push_str("duration_trend: not enough runs to compare\n"),
    }

    match &data.compute {
        Some(c) => t.push_str(&format!(
            "compute: cpu_busy {:.0}%, cpu_wait {:.0}%, mem_avg {:.0}%, mem_p90 {:.0}%\n",
            c.avg_cpu_busy_pct, c.avg_cpu_wait_pct, c.avg_mem_used_pct, c.p90_mem_used_pct
        )),
        None => t.push_str("compute: unavailable\n"),
    }
    if let Some(f) = data.node_family {
        t.push_str(&format!("node_family: {}\n", family_label(f)));
    }

    if data.task_attribution_available {
        if data.task_failures.is_empty() {
            t.push_str("task_failures: none\n");
        } else {
            t.push_str("task_failures:\n");
            for tf in data.task_failures.iter().take(MAX_TASKS_IN_DIGEST) {
                t.push_str(&format!(
                    "  - {}: {} failed of {} runs\n",
                    tf.task_key, tf.failures, tf.total
                ));
            }
        }
    } else {
        t.push_str("task_failures: unreadable\n");
    }

    if data.flags.is_empty() {
        t.push_str("flags: none\n");
    } else {
        t.push_str("flags:\n");
        for f in &data.flags {
            t.push_str(&format!(
                "  - [{}] {}\n",
                severity_label(f.severity),
                f.message
            ));
        }
    }

    match live {
        Some(l) if !l.stages.is_empty() => {
            t.push_str(&format!("stages (from {:?}):\n", l.source));
            for s in l.stages.iter().take(MAX_STAGES_IN_DIGEST) {
                t.push_str(&format!(
                    "  - {}: {} tasks, skew {:.1}x (max {}ms / median {}ms), spilled {} mem + {} \
                     disk bytes\n",
                    s.name,
                    s.num_tasks,
                    s.skew_ratio,
                    s.max_task_duration_ms,
                    s.median_task_duration_ms,
                    s.memory_bytes_spilled,
                    s.disk_bytes_spilled
                ));
            }
        }
        _ => t.push_str("stages: unavailable\n"),
    }

    // Error text gets the remainder of the budget. Head, not tail: the
    // exception type and message lead the blob that `runs::full_output`
    // builds, and that's what distinguishes an OOM from a 403.
    if let Some(err) = failure_output {
        t.push_str("latest_failure_output: |\n");
        let spent = t.len();
        let room = MAX_DIGEST_CHARS.saturating_sub(spent + 32);
        let clean = redact(err.trim());
        let body = if clean.len() > room {
            let cut = clean
                .char_indices()
                .take_while(|(i, _)| *i <= room)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            format!("{}\n…[truncated]", &clean[..cut])
        } else {
            clean
        };
        for line in body.lines() {
            t.push_str("  ");
            t.push_str(line);
            t.push('\n');
        }
    } else {
        t.push_str("latest_failure_output: none\n");
    }

    let hash = fnv1a(t.as_bytes());
    Digest { text: t, hash }
}

/// The instruction wrapped around the digest. Two things earn their
/// place here: the demand that every prescription cite a line from the
/// digest, and the explicit permission to conclude nothing — a model
/// that must produce three fixes will invent three fixes, and confident
/// nonsense about shuffle partitions is worse than an empty pane.
fn build_prompt(digest: &Digest) -> String {
    format!(
        "You are diagnosing a Databricks job from a health digest. The numeric signals have \
         already been analysed by rules; the flags below are those conclusions. Your job is to \
         explain the FAILURE TEXT and synthesise across signals — not to restate the numbers.\n\n\
         Rules:\n\
         - Cite the exact digest line each prescription rests on, in `evidence`.\n\
         - Never recommend a change no line in the digest supports.\n\
         - If the evidence does not identify a cause, say so and return an empty list. That is a \
           correct answer, not a failure.\n\
         - At most 3 prescriptions, most important first. Be concrete: name the setting, node \
           family, or code change.\n\n\
         Reply with JSON only, no prose, no code fences:\n\
         {{\"summary\": \"one sentence\", \"prescriptions\": [{{\"action\": \"...\", \
         \"rationale\": \"...\", \"evidence\": \"...\", \"confidence\": \"high|medium|low\"}}]}}\n\n\
         DIGEST\n{}",
        digest.text
    )
}

/// Standard base64, no line breaks. Hand-rolled to avoid a dependency
/// for twenty lines — see [`ai_query_sql`] for why the prompt travels
/// encoded rather than quoted.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// The prompt travels base64-encoded and is decoded server-side rather
/// than being interpolated as a SQL string literal. Elsewhere in this
/// codebase system-table queries strip quotes from their inputs, which
/// is fine for a job id — but this input is an arbitrary stack trace
/// full of quotes, backslashes and newlines, and stripping them would
/// both mangle the evidence and leave the escaping correctness of the
/// statement resting on that stripping. Encoding removes the question:
/// the payload is `[A-Za-z0-9+/=]` by construction.
fn ai_query_sql(endpoint: &str, prompt: &str) -> String {
    // Serving endpoint names are alphanumeric plus `-`, `_` and `.`;
    // keeping only those is a stronger guarantee than stripping quotes,
    // and a name mangled by this filter fails server-side with a clear
    // "endpoint not found" rather than doing something surprising.
    let endpoint: String = endpoint
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    format!(
        "SELECT ai_query('{endpoint}', decode(unbase64('{}'), 'UTF-8'), \
         modelParameters => named_struct('max_tokens', {MAX_OUTPUT_TOKENS}, 'temperature', 0)) \
         AS verdict",
        base64(prompt.as_bytes())
    )
}

#[derive(Deserialize)]
struct RawPrescription {
    action: String,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    evidence: String,
    #[serde(default)]
    confidence: String,
}

#[derive(Deserialize)]
struct RawVerdict {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    prescriptions: Vec<RawPrescription>,
}

/// Pulls the JSON object out of a model reply. Asking for "JSON only"
/// gets it most of the time; the rest of the time it arrives wrapped in
/// a code fence or trailed by a sentence, so the object is located by
/// its braces rather than trusted to be the whole string.
fn parse_verdict(reply: &str) -> Result<(String, Vec<Prescription>), String> {
    let start = reply
        .find('{')
        .ok_or("model reply contained no JSON object")?;
    let end = reply
        .rfind('}')
        .ok_or("model reply contained no JSON object")?;
    if end <= start {
        return Err("model reply contained no JSON object".to_string());
    }
    let raw: RawVerdict = serde_json::from_str(&reply[start..=end])
        .map_err(|e| format!("could not parse the model's reply: {e}"))?;

    let prescriptions = raw
        .prescriptions
        .into_iter()
        .map(|p| Prescription {
            action: p.action,
            rationale: p.rationale,
            evidence: p.evidence,
            confidence: Confidence::parse(&p.confidence),
        })
        .filter(|p| !p.action.trim().is_empty())
        .collect();

    Ok((raw.summary, prescriptions))
}

/// Runs the digest through `ai_query` on an already-open warehouse and
/// caches the result. Call only on [`Gate::Ask`] — this is the one path
/// in the app that bills.
pub async fn prescribe(
    cli: &DatabricksCli,
    warehouse_id: &str,
    endpoint: &str,
    digest: &Digest,
) -> Result<DoctorVerdict, String> {
    let prompt = build_prompt(digest);
    let table = run_sql(cli, &ai_query_sql(endpoint, &prompt), warehouse_id).await?;

    let reply = table
        .rows
        .first()
        .and_then(|r| r.first())
        .ok_or("the endpoint returned no rows — check that it exists and is serving")?;

    let (summary, prescriptions) = parse_verdict(reply)?;

    let verdict = DoctorVerdict {
        summary,
        prescriptions,
        usage: Usage {
            prompt_tokens_est: (prompt.len() / CHARS_PER_TOKEN) as u32,
            completion_tokens_est: (reply.len() / CHARS_PER_TOKEN) as u32,
            cached: false,
        },
    };
    store_cached(&digest.key(), &verdict);
    Ok(verdict)
}

/// How far back to look for a failed run. Deliberately not `runs::list`,
/// whose limit of 20 is tuned for the run drill-down's visible list: a
/// job that fails once a month puts its failure well outside that
/// window, and missing it made the gate skip a job whose health view was
/// plainly showing a failure — the numbers said something was wrong and
/// the doctor said there was nothing to diagnose. The health report's
/// own window is 30 days, so the scan has to reach at least that far.
const FAILURE_SCAN_LIMIT: &str = "50";

/// Error text of the most recent failed run, for the digest. Two CLI
/// calls and no warehouse time, so this is cheap enough to fetch before
/// gating — which matters, because its presence is what decides whether
/// a job is worth diagnosing at all.
pub async fn latest_failure_output(cli: &DatabricksCli, job_id: &str) -> Option<String> {
    let json = cli
        .run(&[
            "jobs",
            "list-runs",
            "--job-id",
            job_id,
            "--limit",
            FAILURE_SCAN_LIMIT,
        ])
        .await
        .ok()?;
    let run_id = json
        .as_array()?
        .iter()
        .find(|r| jobs::run_status(r) == Status::Failed)
        .and_then(|r| r["run_id"].as_u64())?
        .to_string();
    let (output, _) = runs::full_output(cli, &run_id).await;
    Some(output).filter(|o| !o.trim().is_empty())
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    key: String,
    verdict: DoctorVerdict,
    /// Unix seconds, for eviction order only.
    saved_at: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    #[serde(default)]
    entries: Vec<CacheEntry>,
}

fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("databricks-tui")
            .join("doctor-cache.json"),
    )
}

fn load_cache() -> CacheFile {
    cache_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn load_cached(key: &str) -> Option<DoctorVerdict> {
    load_cache()
        .entries
        .into_iter()
        .find(|e| e.key == key)
        .map(|e| e.verdict)
}

/// Best-effort, like `Config::save` — a cache that can't be written
/// costs another call next time, which is not worth failing a diagnosis
/// over.
fn store_cached(key: &str, verdict: &DoctorVerdict) {
    let Some(path) = cache_path() else {
        return;
    };
    let mut cache = load_cache();
    cache.entries.retain(|e| e.key != key);
    cache.entries.push(CacheEntry {
        key: key.to_string(),
        verdict: verdict.clone(),
        saved_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    cache.entries.sort_by_key(|e| e.saved_at);
    let len = cache.entries.len();
    if len > MAX_CACHE_ENTRIES {
        cache.entries.drain(..len - MAX_CACHE_ENTRIES);
    }

    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        config::restrict(dir, 0o700);
    }
    if let Ok(json) = serde_json::to_string_pretty(&cache) {
        let _ = std::fs::write(&path, json);
        config::restrict(&path, 0o600);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetchers::job_health::{ComputePressure, HealthFlag, TaskFailure};
    use crate::fetchers::spark_live::{DiagSource, SparkLiveData, StageDiag};

    const ENDPOINT: Option<&str> = Some("databricks-claude-haiku");

    /// A job with nothing wrong: enough runs, no failures, no flags.
    fn healthy() -> JobHealthData {
        JobHealthData {
            window_days: 30,
            days: Vec::new(),
            success_rate: 100.0,
            total_runs: 40,
            failed_runs: 0,
            duration_points: Vec::new(),
            duration_trend_pct: Some(2.0),
            task_failures: Vec::new(),
            task_attribution_available: true,
            compute: Some(ComputePressure {
                avg_cpu_busy_pct: 55.0,
                avg_cpu_wait_pct: 4.0,
                avg_mem_used_pct: 50.0,
                p90_mem_used_pct: 61.0,
            }),
            node_family: Some(NodeFamily::General),
            flags: Vec::new(),
            scoped: true,
        }
    }

    fn flagged(severity: FlagSeverity) -> JobHealthData {
        let mut data = healthy();
        data.success_rate = 72.0;
        data.failed_runs = 11;
        data.flags = vec![HealthFlag {
            severity,
            message: "success rate is only 72% over the last 30 days".to_string(),
        }];
        data
    }

    fn spilling_stage() -> SparkLiveData {
        SparkLiveData {
            app_id: "app-1".to_string(),
            run_id: "99".to_string(),
            source: DiagSource::EventLog,
            stages: vec![StageDiag {
                stage_id: 4,
                name: "shuffle at Join.scala:88".to_string(),
                num_tasks: 200,
                max_task_duration_ms: 91_000,
                median_task_duration_ms: 4_000,
                skew_ratio: 22.75,
                memory_bytes_spilled: 8_000_000_000,
                disk_bytes_spilled: 3_000_000_000,
            }],
        }
    }

    // ---- gating: the part that decides whether money gets spent ----

    #[test]
    fn an_unconfigured_endpoint_never_reaches_the_model() {
        let gate = gate(
            None,
            "etl",
            &flagged(FlagSeverity::Critical),
            None,
            Some("boom"),
        );
        assert!(matches!(gate, Gate::Skip(msg) if msg.contains("doctor is off")));
    }

    #[test]
    fn an_empty_endpoint_string_is_off_too_not_a_call_to_an_empty_name() {
        let gate = gate(
            Some(""),
            "etl",
            &flagged(FlagSeverity::Critical),
            None,
            None,
        );
        assert!(matches!(gate, Gate::Skip(_)));
    }

    /// 34 of 35 runs succeeded, so no threshold fired — but one run
    /// genuinely failed, and the health view says so. This is the shape
    /// that made the doctor look broken.
    fn one_old_failure() -> JobHealthData {
        let mut data = healthy();
        data.total_runs = 35;
        data.failed_runs = 1;
        data.success_rate = 97.1;
        data
    }

    #[test]
    fn a_healthy_job_costs_nothing() {
        let gate = gate(ENDPOINT, "etl", &healthy(), None, None);
        assert!(matches!(gate, Gate::Skip(msg) if msg.contains("nothing to diagnose")));
    }

    /// Info flags are already their own prescription — "cluster looks
    /// over-provisioned" needs no model to explain it.
    #[test]
    fn an_info_only_flag_is_not_worth_a_call() {
        let mut data = healthy();
        data.flags = vec![HealthFlag {
            severity: FlagSeverity::Info,
            message: "average CPU utilization is only 12%".to_string(),
        }];
        assert!(matches!(
            gate(ENDPOINT, "etl", &data, None, None),
            Gate::Skip(_)
        ));
    }

    #[test]
    fn a_warn_or_critical_flag_is_worth_a_call() {
        for severity in [FlagSeverity::Warn, FlagSeverity::Critical] {
            let gate = gate(ENDPOINT, "etl", &flagged(severity), None, None);
            assert!(matches!(gate, Gate::Ask(_)), "{severity:?} should ask");
        }
    }

    /// Whitespace-only output is the same as no output — it must not be
    /// what tips a thin-sample job into being diagnosed.
    #[test]
    fn blank_failure_text_does_not_count_as_evidence() {
        let gate = gate(ENDPOINT, "etl", &healthy(), None, Some("   \n  "));
        assert!(matches!(gate, Gate::Skip(_)));
    }

    /// The refusal is right — with no error text there is nothing to add
    /// beyond the numbers already on screen — but it must not claim the
    /// job is clean while the view above it shows a failure.
    #[test]
    fn an_unreadable_failure_is_not_reported_as_a_clean_bill_of_health() {
        let gate = gate(ENDPOINT, "etl", &one_old_failure(), None, None);
        let Gate::Skip(msg) = gate else {
            panic!("no error text and no flag — should not ask");
        };
        assert!(msg.contains("1 of 35 runs failed"), "{msg}");
        assert!(
            !msg.contains("nothing to diagnose"),
            "a job with a failure must not be described as having nothing wrong: {msg}"
        );
    }

    /// The same job once the deeper run scan actually reaches the
    /// failure — this is the case the doctor exists for.
    #[test]
    fn a_readable_failure_is_worth_a_call_even_at_97_percent_success() {
        let gate = gate(
            ENDPOINT,
            "etl",
            &one_old_failure(),
            None,
            Some("java.lang.OutOfMemoryError: Java heap space"),
        );
        assert!(matches!(gate, Gate::Ask(_)));
    }

    #[test]
    fn a_job_that_never_failed_still_says_nothing_to_diagnose() {
        let gate = gate(ENDPOINT, "etl", &healthy(), None, None);
        assert!(matches!(gate, Gate::Skip(msg) if msg.contains("nothing to diagnose")));
    }

    #[test]
    fn a_thin_run_sample_is_skipped_unless_there_is_error_text_to_read() {
        let mut data = flagged(FlagSeverity::Critical);
        data.total_runs = 3;
        data.failed_runs = 1;

        let statistical = gate(ENDPOINT, "etl", &data, None, None);
        assert!(
            matches!(statistical, Gate::Skip(msg) if msg.contains("too little history")),
            "3 runs and no error text is not a diagnosis"
        );

        let with_error = gate(
            ENDPOINT,
            "etl",
            &data,
            None,
            Some("java.lang.OutOfMemoryError: Java heap space"),
        );
        assert!(
            matches!(with_error, Gate::Ask(_)),
            "error text is direct evidence and stands on its own"
        );
    }

    /// Spill/skew live in `cross_signal_flags`, which is computed at
    /// render time and never lands in `data.flags` — so the gate has to
    /// look at the stages itself or it would skip a spilling job.
    #[test]
    fn stage_spill_alone_is_enough_to_ask() {
        let gate = gate(ENDPOINT, "etl", &healthy(), Some(&spilling_stage()), None);
        assert!(matches!(gate, Gate::Ask(_)));
    }

    #[test]
    fn clean_stages_do_not_trigger_a_call() {
        let mut live = spilling_stage();
        live.stages[0].memory_bytes_spilled = 0;
        live.stages[0].disk_bytes_spilled = 0;
        live.stages[0].skew_ratio = 1.2;
        assert!(matches!(
            gate(ENDPOINT, "etl", &healthy(), Some(&live), None),
            Gate::Skip(_)
        ));
    }

    // ---- digest: stability is what makes the cache work ----

    #[test]
    fn identical_evidence_hashes_identically_so_a_refresh_is_free() {
        let data = flagged(FlagSeverity::Critical);
        let a = build_digest("etl", &data, Some(&spilling_stage()), Some("boom"));
        let b = build_digest("etl", &data, Some(&spilling_stage()), Some("boom"));
        assert_eq!(a.text, b.text);
        assert_eq!(a.hash, b.hash);
    }

    #[test]
    fn a_changed_signal_changes_the_hash() {
        let base = build_digest("etl", &flagged(FlagSeverity::Critical), None, None);
        let mut worse = flagged(FlagSeverity::Critical);
        worse.failed_runs += 1;
        let after = build_digest("etl", &worse, None, None);
        assert_ne!(base.hash, after.hash);
    }

    /// Rounding is applied in the digest itself: a success rate that
    /// drifts in the seventh decimal between two refreshes of the same
    /// underlying runs must not look like new evidence.
    #[test]
    fn imperceptible_float_drift_does_not_bill_twice() {
        let mut a = flagged(FlagSeverity::Critical);
        a.success_rate = 72.000_000_1;
        let mut b = flagged(FlagSeverity::Critical);
        b.success_rate = 72.000_000_9;
        assert_eq!(
            build_digest("etl", &a, None, None).hash,
            build_digest("etl", &b, None, None).hash
        );
    }

    #[test]
    fn a_huge_stack_trace_is_truncated_to_the_budget() {
        let giant = "x".repeat(500_000);
        let digest = build_digest("etl", &flagged(FlagSeverity::Critical), None, Some(&giant));
        assert!(
            digest.text.len() <= MAX_DIGEST_CHARS,
            "digest was {} chars, over the {MAX_DIGEST_CHARS} budget",
            digest.text.len()
        );
        assert!(digest.text.contains("[truncated]"));
    }

    #[test]
    fn every_derived_signal_reaches_the_digest() {
        let mut data = flagged(FlagSeverity::Critical);
        data.task_failures = vec![TaskFailure {
            task_key: "load_dim".to_string(),
            failures: 9,
            total: 40,
        }];
        let text = build_digest("nightly_etl", &data, Some(&spilling_stage()), Some("OOM")).text;
        for expected in [
            "nightly_etl",
            "success",
            "duration_trend",
            "compute:",
            "node_family: general-purpose",
            "load_dim",
            "[critical]",
            "shuffle at Join.scala:88",
            "latest_failure_output",
        ] {
            assert!(
                text.contains(expected),
                "digest is missing {expected:?}:\n{text}"
            );
        }
    }

    #[test]
    fn missing_optional_signals_are_stated_not_silently_dropped() {
        let mut data = healthy();
        data.compute = None;
        data.node_family = None;
        data.task_attribution_available = false;
        data.flags = vec![HealthFlag {
            severity: FlagSeverity::Warn,
            message: "runs have gotten 40% slower".to_string(),
        }];
        let text = build_digest("etl", &data, None, None).text;
        assert!(text.contains("compute: unavailable"));
        assert!(text.contains("task_failures: unreadable"));
        assert!(text.contains("stages: unavailable"));
        assert!(text.contains("latest_failure_output: none"));
    }

    // ---- redaction ----

    #[test]
    fn credentials_in_log_text_are_blanked_before_they_travel() {
        // Assembled at runtime: a token-shaped literal in the source
        // trips GitHub's secret scanner on push, and a test fixture is
        // not worth teaching people to click through that warning.
        let fake = format!("{}{}", "da", "pi0123456789abcdef0123456789abcdef");
        let jwt = format!("{}{}", "eyJhbGciOi", ".signature");
        let out = redact(&format!("connect token={fake} header Bearer {jwt}"));
        assert!(!out.contains(&fake), "{out}");
        assert!(!out.contains(&jwt), "{out}");
        assert!(out.contains("connect") && out.contains("header"), "{out}");
    }

    #[test]
    fn redaction_survives_multiple_spaces_after_bearer() {
        assert!(!redact("Authorization: Bearer   sekrit").contains("sekrit"));
    }

    #[test]
    fn ordinary_words_starting_with_dapi_are_left_alone() {
        // Short enough not to be a token — mangling real log text would
        // cost us the evidence the call exists to read.
        assert_eq!(redact("dapiservice failed"), "dapiservice failed");
    }

    // ---- the SQL payload ----

    #[test]
    fn base64_matches_the_reference_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// The prompt carries an arbitrary stack trace. Encoded, it cannot
    /// close the surrounding string literal no matter what it contains.
    #[test]
    fn a_stack_trace_full_of_quotes_cannot_break_out_of_the_statement() {
        let nasty = "'; DROP TABLE x; -- \\ \"quoted\"\nnewline";
        let sql = ai_query_sql("my-endpoint", nasty);
        assert!(!sql.contains("DROP TABLE"));
        assert!(!sql.contains('\n'));
        let payload = sql
            .split("unbase64('")
            .nth(1)
            .and_then(|s| s.split('\'').next())
            .expect("payload");
        assert!(payload
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)));
    }

    #[test]
    fn a_quoted_endpoint_name_cannot_inject_either() {
        let sql = ai_query_sql("ep', 'x", "hi");
        assert!(sql.starts_with("SELECT ai_query('epx',"), "{sql}");
    }

    #[test]
    fn the_output_cap_is_carried_into_the_statement() {
        let sql = ai_query_sql("ep", "hi");
        assert!(sql.contains(&format!("'max_tokens', {MAX_OUTPUT_TOKENS}")));
        assert!(sql.contains("'temperature', 0"));
    }

    // ---- reply parsing ----

    #[test]
    fn a_fenced_reply_still_parses() {
        let reply = "Sure!\n```json\n{\"summary\": \"OOM on the join\", \"prescriptions\": \
                     [{\"action\": \"switch to r6gd.2xlarge\", \"rationale\": \"heap exhausted\", \
                     \"evidence\": \"mem_p90 97%\", \"confidence\": \"high\"}]}\n```\nHope that helps.";
        let (summary, rx) = parse_verdict(reply).expect("should parse");
        assert_eq!(summary, "OOM on the join");
        assert_eq!(rx.len(), 1);
        assert_eq!(rx[0].confidence, Confidence::High);
        assert_eq!(rx[0].evidence, "mem_p90 97%");
    }

    /// "I don't know" is a correct answer and must survive parsing —
    /// the prompt explicitly permits it.
    #[test]
    fn an_empty_prescription_list_is_a_valid_verdict() {
        let (summary, rx) =
            parse_verdict("{\"summary\": \"no clear cause\", \"prescriptions\": []}").unwrap();
        assert_eq!(summary, "no clear cause");
        assert!(rx.is_empty());
    }

    #[test]
    fn prescriptions_without_an_action_are_dropped() {
        let reply = "{\"prescriptions\": [{\"action\": \"  \"}, {\"action\": \"repartition\"}]}";
        let (_, rx) = parse_verdict(reply).unwrap();
        assert_eq!(rx.len(), 1);
        assert_eq!(rx[0].action, "repartition");
        // Absent confidence is not high confidence.
        assert_eq!(rx[0].confidence, Confidence::Medium);
    }

    #[test]
    fn a_reply_with_no_json_is_an_error_not_a_panic() {
        assert!(parse_verdict("the endpoint is warming up").is_err());
        assert!(parse_verdict("").is_err());
        assert!(parse_verdict("}{").is_err());
    }

    #[test]
    fn the_prompt_carries_the_digest_and_the_permission_to_find_nothing() {
        let digest = build_digest("etl", &flagged(FlagSeverity::Critical), None, Some("OOM"));
        let prompt = build_prompt(&digest);
        assert!(prompt.contains(&digest.text));
        assert!(prompt.contains("empty list"));
        assert!(prompt.contains("evidence"));
    }
}
