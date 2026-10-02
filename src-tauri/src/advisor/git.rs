//! Task-level cost: which commit did the tokens go into? [BACKEND]
//!
//! Strictly opt-in (`gitAttribution`, default off): while the setting is off no
//! process is spawned and nothing is read. When it is on, the Commits view of
//! the History tab asks for one project at a time.
//!
//! ## What is run, where, and what is read
//!
//! * Only for a `cwd` that appears **verbatim** in `usage_events` (the user's
//!   own recorded project list) *and* is an existing directory with a `.git`
//!   entry in it or in one of its ancestors. Anything else is refused before a
//!   process starts, so the command cannot be pointed at an arbitrary path.
//! * Exactly one command, run with that `cwd` as the working directory:
//!   `git --no-pager log --no-merges --no-color --no-ext-diff --no-show-signature
//!    -n 500 --since=… --until=… --format=%H%x09%ct%x09%s`.
//!   That is commit hash, committer time and subject: no diff, no file name, no
//!   author, no file contents. The environment is cleaned of `GIT_DIR` /
//!   `GIT_WORK_TREE` / `GIT_INDEX_FILE`, prompts are disabled, the run is
//!   killed after [`GIT_TIMEOUT`] and its output is capped.
//! * `git` is found through `PATH` (on Windows too); when it is missing the
//!   view says so instead of failing.
//!
//! Subjects stay on this machine: they are shown in the dashboard and are not
//! exported, logged or sent anywhere.
//!
//! ## Attribution heuristic (an estimate, not a measurement)
//!
//! Commits of the current branch (merges skipped) are ordered by committer
//! time. The usage events of that project (`usage_events.cwd == project`) that
//! fall in `(previous commit, this commit]` are attributed to this commit, but
//! never reaching back further than [`MAX_GAP_MS`] (6 h) — a long idle gap is
//! not "work towards this commit". Events outside every such interval (the
//! tail after the last commit = uncommitted work, or work older than the gap
//! cap) are reported as *unattributed*. Known limits:
//!
//! * a commit made after a day of work only gets the last 6 h;
//! * work done in another checkout/worktree, or with a different recorded
//!   `cwd`, is not counted for this project;
//! * parallel sessions and amended/rebased commits (committer time changes)
//!   shift tokens between neighbouring commits;
//! * committer clocks that go backwards produce empty intervals.
//!
//! Costs use the same price table as the rest of the app and are estimates.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::commands::pricing;
use crate::commands::store::{usage, Db};
use crate::model::{PricingTable, TokenTotals};

/// Most commits one request returns (`git log -n`).
pub const MAX_COMMITS: usize = 500;
/// `git` is killed after this long.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Tokens older than this before a commit are not attributed to it.
pub const MAX_GAP_MS: i64 = 6 * 3_600_000;
/// Output cap; 500 lines of subjects are far below it.
const MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(300);
const CACHE_ENTRIES: usize = 16;
/// Window ends are rounded to this so a re-opened view hits the cache.
const BUCKET_MS: i64 = 300_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitsQuery {
    /// Exact `cwd` as listed in the history's project filter.
    pub project: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub provider: Option<String>,
    /// Bypass the cached `git log` result.
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommitsStatus {
    Ok,
    /// `gitAttribution` is off: nothing was run.
    Disabled,
    /// The project is not one the usage log recorded.
    UnknownProject,
    /// The directory is gone or is not inside a git repository.
    NotARepo,
    /// No `git` executable on `PATH`.
    GitMissing,
    /// git ran and failed or timed out; `message` has the first stderr line.
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitRow {
    pub hash: String,
    pub short_hash: String,
    /// RFC 3339 with the local offset (committer time)
    pub ts: String,
    pub subject: String,
    /// start of the attribution interval (RFC 3339, local offset)
    pub window_start: String,
    /// distinct session ids with at least one request in the interval
    pub sessions: i64,
    #[serde(flatten)]
    pub totals: TokenTotals,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitsResult {
    pub status: CommitsStatus,
    pub message: Option<String>,
    /// newest first
    pub commits: Vec<CommitRow>,
    /// project usage in the range that no commit claimed (uncommitted work,
    /// or older than the 6 h gap cap)
    pub unattributed: TokenTotals,
    /// more commits existed than the cap of [`MAX_COMMITS`]
    pub truncated: bool,
    /// the `git log` part came from the cache
    pub cached: bool,
}

impl CommitsResult {
    fn bare(status: CommitsStatus, message: Option<String>) -> Self {
        CommitsResult {
            status,
            message,
            commits: Vec::new(),
            unattributed: TokenTotals::default(),
            truncated: false,
            cached: false,
        }
    }
}

// ---------- git log ----------

/// One commit as `git log` printed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawCommit {
    pub hash: String,
    /// committer time, unix ms
    pub ts_ms: i64,
    pub subject: String,
}

/// Parse `%H%x09%ct%x09%s` lines; malformed lines are skipped. The result is
/// oldest first (stable for equal timestamps).
pub fn parse_git_log(out: &str) -> Vec<RawCommit> {
    let mut commits: Vec<RawCommit> = out
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim_end_matches('\r').splitn(3, '\t');
            let hash = parts.next()?.trim();
            let ct: i64 = parts.next()?.trim().parse().ok()?;
            let subject = parts.next().unwrap_or("").trim();
            let hex = hash.len() == 40 || hash.len() == 64;
            if !hex || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || ct < 0 {
                return None;
            }
            Some(RawCommit {
                hash: hash.to_ascii_lowercase(),
                ts_ms: ct.checked_mul(1000)?,
                subject: subject.chars().take(300).collect(),
            })
        })
        .collect();
    // git prints newest first
    commits.reverse();
    commits.sort_by_key(|c| c.ts_ms);
    commits
}

#[derive(Debug)]
pub enum GitError {
    Missing,
    Timeout,
    Failed(String),
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

/// Run the one allowed `git log` in `cwd`. `since_s` / `until_s` are unix seconds.
pub fn run_git_log(cwd: &Path, since_s: i64, until_s: i64) -> Result<String, GitError> {
    use std::process::{Command, Stdio};
    let iso = |s: i64| {
        chrono::DateTime::from_timestamp(s, 0)
            .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_default()
    };
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .args([
            "--no-pager",
            "log",
            "--no-merges",
            "--no-color",
            "--no-ext-diff",
            "--no-show-signature",
            "-n",
        ])
        .arg(MAX_COMMITS.to_string())
        .arg(format!("--since={}", iso(since_s)))
        .arg(format!("--until={}", iso(until_s)))
        .arg("--format=%H%x09%ct%x09%s")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::Missing
        } else {
            GitError::Failed(e.to_string())
        }
    })?;
    let drain = |pipe: Option<Box<dyn Read + Send>>, keep: u64| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = pipe {
                let _ = (&mut r).take(keep).read_to_end(&mut buf);
                let _ = std::io::copy(&mut r, &mut std::io::sink());
            }
            buf
        })
    };
    let out_reader = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        MAX_OUTPUT_BYTES,
    );
    let err_reader = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        8 * 1024,
    );
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > GIT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::Timeout);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(GitError::Failed(e.to_string())),
        }
    };
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    if status.success() {
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    let message = first_line(&err);
    // a fresh repository without a commit is an empty history, not an error
    if message.contains("does not have any commits yet") {
        return Ok(String::new());
    }
    Err(GitError::Failed(message))
}

/// `cwd` is an existing directory with a `.git` entry in it or above it.
pub fn inside_git_repo(cwd: &Path) -> bool {
    cwd.is_dir()
        && cwd
            .ancestors()
            .take(40)
            .any(|dir| dir.join(".git").exists())
}

// ---------- cache ----------

type CacheKey = (String, i64, i64);
static LOG_CACHE: Mutex<Vec<(CacheKey, Instant, Vec<RawCommit>)>> = Mutex::new(Vec::new());

fn cache_get(key: &CacheKey) -> Option<Vec<RawCommit>> {
    let mut cache = LOG_CACHE.lock();
    cache.retain(|(_, at, _)| at.elapsed() < CACHE_TTL);
    cache
        .iter()
        .find(|(k, _, _)| k == key)
        .map(|(_, _, commits)| commits.clone())
}

fn cache_put(key: CacheKey, commits: Vec<RawCommit>) {
    let mut cache = LOG_CACHE.lock();
    cache.retain(|(k, _, _)| *k != key);
    while cache.len() >= CACHE_ENTRIES {
        cache.remove(0);
    }
    cache.push((key, Instant::now(), commits));
}

// ---------- attribution ----------

/// One `usage_events` row, reduced to what attribution needs.
#[derive(Clone, Debug)]
pub struct EventRow {
    pub ts_ms: i64,
    pub model: String,
    pub totals: TokenTotals,
    pub session_id: String,
}

#[derive(Default)]
struct Sum {
    totals: TokenTotals,
    cost: f64,
    priced: i64,
    sessions: BTreeSet<String>,
}

impl Sum {
    fn add(&mut self, e: &EventRow, pricing: &PricingTable) {
        let t = &mut self.totals;
        t.input_tokens += e.totals.input_tokens;
        t.cache_write_tokens += e.totals.cache_write_tokens;
        t.cache_read_tokens += e.totals.cache_read_tokens;
        t.output_tokens += e.totals.output_tokens;
        t.reasoning_tokens += e.totals.reasoning_tokens;
        t.total_tokens += e.totals.total_tokens;
        t.requests += 1;
        match pricing::estimate_cost(pricing, &e.model, &e.totals) {
            Some(c) => {
                self.cost += c;
                self.priced += 1;
            }
            None => t.unpriced_requests += 1,
        }
        if !e.session_id.is_empty() {
            self.sessions.insert(e.session_id.clone());
        }
    }

    /// Same cost semantics as the rest of the history: the total is `None` as
    /// soon as one request had no price, the known subtotal is kept.
    fn finish(mut self) -> (TokenTotals, i64) {
        if self.priced > 0 {
            self.totals.known_cost_usd = Some(self.cost);
            if self.totals.unpriced_requests == 0 {
                self.totals.estimated_cost_usd = Some(self.cost);
            }
        }
        (self.totals, self.sessions.len() as i64)
    }
}

/// The attribution itself. `commits` oldest first; `events` any order. Only
/// commits with `visible_from <= ts <= visible_to` are returned (the earlier
/// ones merely bound the intervals), and only events inside the visible range
/// count as unattributed.
pub fn attribute(
    commits: &[RawCommit],
    events: &[EventRow],
    visible_from_ms: i64,
    visible_to_ms: i64,
    pricing: &PricingTable,
) -> (Vec<CommitRow>, TokenTotals) {
    let mut sums: Vec<Sum> = commits.iter().map(|_| Sum::default()).collect();
    let mut loose = Sum::default();
    for e in events {
        // first commit at or after the event
        let idx = commits.partition_point(|c| c.ts_ms < e.ts_ms);
        let claimed = commits.get(idx).is_some_and(|c| {
            let prev = if idx == 0 {
                i64::MIN
            } else {
                commits[idx - 1].ts_ms
            };
            e.ts_ms > prev && e.ts_ms > c.ts_ms - MAX_GAP_MS
        });
        if claimed {
            sums[idx].add(e, pricing);
        } else if e.ts_ms >= visible_from_ms && e.ts_ms <= visible_to_ms {
            loose.add(e, pricing);
        }
    }
    let mut rows = Vec::new();
    for (i, (c, sum)) in commits.iter().zip(sums).enumerate() {
        if c.ts_ms < visible_from_ms || c.ts_ms > visible_to_ms {
            continue;
        }
        let prev = if i == 0 {
            i64::MIN
        } else {
            commits[i - 1].ts_ms
        };
        let start = prev.max(c.ts_ms - MAX_GAP_MS);
        let (totals, sessions) = sum.finish();
        rows.push(CommitRow {
            hash: c.hash.clone(),
            short_hash: c.hash.chars().take(8).collect(),
            ts: usage::local_rfc3339(c.ts_ms),
            subject: c.subject.clone(),
            window_start: usage::local_rfc3339(start),
            sessions,
            totals,
        });
    }
    rows.reverse();
    (rows, loose.finish().0)
}

// ---------- the request ----------

fn project_is_recorded(db: &Db, project: &str) -> anyhow::Result<bool> {
    let conn = db.lock();
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM usage_events WHERE cwd = ?1)",
        [project],
        |r| r.get(0),
    )?)
}

fn load_events(
    db: &Db,
    project: &str,
    provider: Option<&str>,
    from: i64,
    to: i64,
) -> anyhow::Result<Vec<EventRow>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT ts, model, input_tokens, cache_write_tokens, cache_read_tokens,
                output_tokens, reasoning_tokens, total_tokens, COALESCE(session_id, '')
         FROM usage_events
         WHERE cwd = ?1 AND ts >= ?2 AND ts < ?3 AND (?4 IS NULL OR provider = ?4)
         ORDER BY ts",
    )?;
    let rows = stmt.query_map(rusqlite::params![project, from, to, provider], |r| {
        Ok(EventRow {
            ts_ms: r.get(0)?,
            model: r.get(1)?,
            totals: TokenTotals {
                input_tokens: r.get(2)?,
                cache_write_tokens: r.get(3)?,
                cache_read_tokens: r.get(4)?,
                output_tokens: r.get(5)?,
                reasoning_tokens: r.get(6)?,
                total_tokens: r.get(7)?,
                ..TokenTotals::default()
            },
            session_id: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The Commits view's data. `git` is injected so tests need no repository.
/// `enabled` is `Settings.git_attribution`: when false nothing at all happens.
pub fn commits_for_project(
    db: &Db,
    pricing: &PricingTable,
    enabled: bool,
    q: &CommitsQuery,
    git: &dyn Fn(&Path, i64, i64) -> Result<String, GitError>,
) -> anyhow::Result<CommitsResult> {
    if !enabled {
        return Ok(CommitsResult::bare(CommitsStatus::Disabled, None));
    }
    let (from, to) = usage::query_range(&q.from, &q.to)?;
    if q.project.is_empty() || !project_is_recorded(db, &q.project)? {
        return Ok(CommitsResult::bare(CommitsStatus::UnknownProject, None));
    }
    let cwd = Path::new(&q.project);
    if !inside_git_repo(cwd) {
        return Ok(CommitsResult::bare(CommitsStatus::NotARepo, None));
    }
    // commits just before the range bound the first visible interval
    let since_ms = (from - MAX_GAP_MS).div_euclid(BUCKET_MS) * BUCKET_MS;
    let until_ms = (to + BUCKET_MS - 1).div_euclid(BUCKET_MS) * BUCKET_MS;
    let key = (q.project.clone(), since_ms, until_ms);
    let cached = if q.refresh { None } else { cache_get(&key) };
    let was_cached = cached.is_some();
    let commits = match cached {
        Some(commits) => commits,
        None => match git(cwd, since_ms / 1000, until_ms / 1000) {
            Ok(out) => {
                let commits = parse_git_log(&out);
                cache_put(key, commits.clone());
                commits
            }
            Err(GitError::Missing) => {
                return Ok(CommitsResult::bare(CommitsStatus::GitMissing, None))
            }
            Err(GitError::Timeout) => {
                return Ok(CommitsResult::bare(
                    CommitsStatus::Error,
                    Some(format!(
                        "git did not answer within {} s",
                        GIT_TIMEOUT.as_secs()
                    )),
                ))
            }
            Err(GitError::Failed(m)) => {
                return Ok(CommitsResult::bare(CommitsStatus::Error, Some(m)))
            }
        },
    };
    let truncated = commits.len() >= MAX_COMMITS;
    // events up to `to` (inclusive of a commit at exactly `to`)
    let events = load_events(
        db,
        &q.project,
        q.provider.as_deref(),
        from - MAX_GAP_MS,
        to + 1,
    )?;
    let (rows, unattributed) = attribute(&commits, &events, from, to, pricing);
    Ok(CommitsResult {
        status: CommitsStatus::Ok,
        message: None,
        commits: rows,
        unattributed,
        truncated,
        cached: was_cached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::store::{insert_usage_events, UsageEvent};

    const T0: i64 = 1_789_430_400_000; // 2026-09-15T00:00:00Z
    const H: i64 = 3_600_000;
    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccc";

    fn commit(hash: &str, ts_ms: i64, subject: &str) -> RawCommit {
        RawCommit {
            hash: hash.into(),
            ts_ms,
            subject: subject.into(),
        }
    }

    fn event(ts_ms: i64, tokens: i64, session: &str) -> EventRow {
        EventRow {
            ts_ms,
            model: "claude-sonnet-4-5".into(),
            totals: TokenTotals {
                input_tokens: tokens,
                total_tokens: tokens,
                ..TokenTotals::default()
            },
            session_id: session.into(),
        }
    }

    #[test]
    fn parses_hash_time_and_subject_and_skips_garbage() {
        let out = format!(
            "{HASH_B}\t{}\tfix: tab\tinside subject\n\
             not-a-hash\t100\tx\n\
             {HASH_A}\tNaN\ty\n\
             \n\
             {HASH_A}\t{}\tfeat: first\r\n",
            (T0 + H) / 1000,
            T0 / 1000
        );
        let commits = parse_git_log(&out);
        assert_eq!(commits.len(), 2);
        // oldest first
        assert_eq!(commits[0].hash, HASH_A);
        assert_eq!(commits[0].ts_ms, T0);
        assert_eq!(commits[0].subject, "feat: first");
        assert_eq!(commits[1].subject, "fix: tab\tinside subject");
        assert!(parse_git_log("").is_empty());
    }

    #[test]
    fn tokens_go_to_the_next_commit_within_the_gap() {
        let commits = [
            commit(HASH_A, T0, "first"),
            commit(HASH_B, T0 + 2 * H, "second"),
            commit(HASH_C, T0 + 10 * H, "third"),
        ];
        let events = [
            event(T0 - H, 100, "s0"),         // before the first commit, within 6 h
            event(T0, 10, "s0"),              // exactly at the first commit: still its own
            event(T0 + H, 200, "s1"),         // between first and second
            event(T0 + 2 * H, 20, "s1"),      // at the second commit
            event(T0 + 4 * H + 1, 400, "s2"), // just inside the third commit's 6 h cap
            event(T0 + 5 * H, 800, "s2"),     // 5 h before the third: inside its 6 h cap
            event(T0 + 3 * H, 1600, "s3"),    // 7 h before the third: older than the gap cap
            event(T0 + 11 * H, 3200, "s4"),   // after the last commit: uncommitted work
        ];
        let (rows, loose) = attribute(
            &commits,
            &events,
            T0 - 24 * H,
            T0 + 24 * H,
            &PricingTable::default(),
        );
        // newest first
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].subject, "third");
        assert_eq!(rows[0].totals.total_tokens, 400 + 800);
        assert_eq!(rows[0].sessions, 1);
        assert_eq!(rows[1].totals.total_tokens, 200 + 20);
        assert_eq!(rows[2].totals.total_tokens, 100 + 10);
        assert_eq!(rows[2].sessions, 1);
        assert_eq!(loose.total_tokens, 1600 + 3200);
        assert_eq!(loose.requests, 2);
        // nothing is counted twice
        let attributed: i64 = rows.iter().map(|r| r.totals.total_tokens).sum();
        assert_eq!(
            attributed + loose.total_tokens,
            events.iter().map(|e| e.totals.total_tokens).sum::<i64>()
        );
    }

    #[test]
    fn commits_outside_the_range_only_bound_the_intervals() {
        let commits = [
            commit(HASH_A, T0, "before range"),
            commit(HASH_B, T0 + 2 * H, "in range"),
        ];
        let events = [event(T0 + H, 50, "s"), event(T0 + 2 * H - 1, 70, "s")];
        // the range starts after the first commit
        let (rows, loose) = attribute(
            &commits,
            &events,
            T0 + H / 2,
            T0 + 3 * H,
            &PricingTable::default(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subject, "in range");
        assert_eq!(rows[0].totals.total_tokens, 120);
        assert_eq!(loose.total_tokens, 0);
        // events before the visible start that no visible commit claims are not "loose"
        let events = [event(T0 - 10 * H, 5, "old")];
        let (_, loose) = attribute(
            &commits,
            &events,
            T0 + H / 2,
            T0 + 3 * H,
            &PricingTable::default(),
        );
        assert_eq!(loose.total_tokens, 0);
    }

    #[test]
    fn same_second_commits_leave_the_later_one_empty() {
        let commits = [commit(HASH_A, T0, "a"), commit(HASH_B, T0, "b")];
        let events = [event(T0 - 1, 9, "s")];
        let (rows, _) = attribute(&commits, &events, T0 - H, T0 + H, &PricingTable::default());
        assert_eq!(rows[1].subject, "a");
        assert_eq!(rows[1].totals.total_tokens, 9);
        assert_eq!(rows[0].totals.total_tokens, 0);
    }

    #[test]
    fn costs_are_estimates_and_unpriced_requests_leave_the_total_unknown() {
        let table = pricing::default_table();
        let mut e = event(T0 - H, 1_000_000, "s");
        e.totals.input_tokens = 1_000_000;
        let mut unknown = event(T0 - 2 * H, 10, "s");
        unknown.model = "mystery-model".into();
        let commits = [commit(HASH_A, T0, "a")];
        let (rows, _) = attribute(&commits, &[e.clone()], T0 - 5 * H, T0 + H, &table);
        assert!(rows[0].totals.estimated_cost_usd.unwrap() > 0.0);
        let (rows, _) = attribute(&commits, &[e, unknown], T0 - 5 * H, T0 + H, &table);
        assert_eq!(rows[0].totals.estimated_cost_usd, None);
        assert!(rows[0].totals.known_cost_usd.unwrap() > 0.0);
        assert_eq!(rows[0].totals.unpriced_requests, 1);
    }

    fn db_with_project(project: &str) -> Db {
        let db = Db::open_in_memory().unwrap();
        insert_usage_events(
            &db,
            &[
                UsageEvent {
                    provider: "claude".into(),
                    model: "claude-sonnet-4-5".into(),
                    ts: T0 - H,
                    input_tokens: 100,
                    total_tokens: 100,
                    request_id: "r1".into(),
                    session_id: Some("s1".into()),
                    cwd: Some(project.into()),
                    ..UsageEvent::default()
                },
                UsageEvent {
                    provider: "claude".into(),
                    model: "claude-sonnet-4-5".into(),
                    ts: T0 - H,
                    input_tokens: 999,
                    total_tokens: 999,
                    request_id: "r2".into(),
                    cwd: Some("/elsewhere".into()),
                    ..UsageEvent::default()
                },
            ],
        )
        .unwrap();
        db
    }

    fn query(project: &str) -> CommitsQuery {
        CommitsQuery {
            project: project.into(),
            from: "2026-09-14T00:00:00Z".into(),
            to: "2026-09-16T00:00:00Z".into(),
            provider: None,
            refresh: true,
        }
    }

    fn explode(_: &Path, _: i64, _: i64) -> Result<String, GitError> {
        panic!("git must not run")
    }

    #[test]
    fn nothing_runs_while_the_setting_is_off_or_for_unrecorded_paths() {
        let dir = crate::commands::test_support::tempdir();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let project = dir.to_string_lossy().to_string();
        let db = db_with_project(&project);
        let table = PricingTable::default();
        let r = commits_for_project(&db, &table, false, &query(&project), &explode).unwrap();
        assert_eq!(r.status, CommitsStatus::Disabled);
        // a real directory that the usage log never recorded is refused
        let other = crate::commands::test_support::tempdir();
        std::fs::create_dir_all(other.join(".git")).unwrap();
        let r = commits_for_project(
            &db,
            &table,
            true,
            &query(&other.to_string_lossy()),
            &explode,
        )
        .unwrap();
        assert_eq!(r.status, CommitsStatus::UnknownProject);
        let r = commits_for_project(&db, &table, true, &query(""), &explode).unwrap();
        assert_eq!(r.status, CommitsStatus::UnknownProject);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    #[test]
    fn a_recorded_folder_that_is_not_a_repository_is_reported() {
        let dir = crate::commands::test_support::tempdir();
        let project = dir.to_string_lossy().to_string();
        let db = db_with_project(&project);
        // NB: the temp dir may itself sit inside a repository on a dev machine;
        // the check walks up, so only assert when it does not.
        if !inside_git_repo(&dir) {
            let r = commits_for_project(
                &db,
                &PricingTable::default(),
                true,
                &query(&project),
                &explode,
            )
            .unwrap();
            assert_eq!(r.status, CommitsStatus::NotARepo);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn git_outcomes_map_to_statuses_and_a_result_is_cached() {
        let dir = crate::commands::test_support::tempdir();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let project = dir.to_string_lossy().to_string();
        let db = db_with_project(&project);
        let table = PricingTable::default();
        let missing = |_: &Path, _: i64, _: i64| Err(GitError::Missing);
        let r = commits_for_project(&db, &table, true, &query(&project), &missing).unwrap();
        assert_eq!(r.status, CommitsStatus::GitMissing);
        let failed =
            |_: &Path, _: i64, _: i64| Err(GitError::Failed("fatal: dubious ownership".into()));
        let r = commits_for_project(&db, &table, true, &query(&project), &failed).unwrap();
        assert_eq!(r.status, CommitsStatus::Error);
        assert_eq!(r.message.as_deref(), Some("fatal: dubious ownership"));
        let slow = |_: &Path, _: i64, _: i64| Err(GitError::Timeout);
        let r = commits_for_project(&db, &table, true, &query(&project), &slow).unwrap();
        assert_eq!(r.status, CommitsStatus::Error);

        let log = format!("{HASH_A}\t{}\tfeat: x\n", T0 / 1000);
        let ok = |_: &Path, _: i64, _: i64| Ok(log.clone());
        let r = commits_for_project(&db, &table, true, &query(&project), &ok).unwrap();
        assert_eq!(r.status, CommitsStatus::Ok);
        assert_eq!(r.commits.len(), 1);
        assert_eq!(
            r.commits[0].totals.total_tokens, 100,
            "only this project's events"
        );
        assert!(!r.cached);
        // second call without `refresh` does not run git again
        let mut q = query(&project);
        q.refresh = false;
        let r = commits_for_project(&db, &table, true, &q, &explode).unwrap();
        assert!(r.cached);
        assert_eq!(r.commits.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn git(dir: &Path, args: &[&str], date: Option<&str>) {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(dir)
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
        if let Some(d) = date {
            cmd.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
        }
        let out = cmd.output().expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn a_real_repository_is_read_and_attributed() {
        if !git_available() {
            eprintln!("git not found; skipping");
            return;
        }
        let dir = crate::commands::test_support::tempdir();
        git(&dir, &["init", "-q"], None);
        // an empty repository is an empty history, not an error
        let empty = run_git_log(&dir, 0, T0 / 1000 + 86_400 * 365).unwrap();
        assert!(parse_git_log(&empty).is_empty());
        git(
            &dir,
            &["commit", "--allow-empty", "-q", "-m", "feat: one"],
            Some(&format!("{} +0000", T0 / 1000)),
        );
        git(
            &dir,
            &["commit", "--allow-empty", "-q", "-m", "fix: two\twith tab"],
            Some(&format!("{} +0000", (T0 + 2 * H) / 1000)),
        );
        // outside the requested window
        git(
            &dir,
            &["commit", "--allow-empty", "-q", "-m", "chore: later"],
            Some(&format!("{} +0000", (T0 + 40 * H) / 1000)),
        );
        let out = run_git_log(&dir, T0 / 1000 - 3600, (T0 + 10 * H) / 1000).unwrap();
        let commits = parse_git_log(&out);
        let subjects: Vec<&str> = commits.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["feat: one", "fix: two\twith tab"]);
        assert_eq!(commits[0].ts_ms, T0);

        // end to end through the real command path
        let project = dir.to_string_lossy().to_string();
        let db = db_with_project(&project);
        insert_usage_events(
            &db,
            &[UsageEvent {
                provider: "claude".into(),
                model: "claude-sonnet-4-5".into(),
                ts: T0 + H,
                input_tokens: 7,
                total_tokens: 7,
                request_id: "r3".into(),
                session_id: Some("s2".into()),
                cwd: Some(project.clone()),
                ..UsageEvent::default()
            }],
        )
        .unwrap();
        let r = commits_for_project(
            &db,
            &PricingTable::default(),
            true,
            &query(&project),
            &|cwd, a, b| run_git_log(cwd, a, b),
        )
        .unwrap();
        assert_eq!(r.status, CommitsStatus::Ok, "{:?}", r.message);
        assert_eq!(r.commits.len(), 2);
        assert_eq!(r.commits[0].subject, "fix: two\twith tab");
        assert_eq!(r.commits[0].totals.total_tokens, 7);
        assert_eq!(r.commits[1].totals.total_tokens, 100);
        assert_eq!(r.commits[1].sessions, 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
