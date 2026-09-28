use super::{
    model::*,
    parser::{self, Context},
};
use crate::{
    commands::{
        pricing,
        store::{
            usage::{local_rfc3339, parse_time_ms},
            Db,
        },
    },
    model::{ModelVariant, PricingTable, SessionRow, TokenTotals},
};
use anyhow::{Context as _, Result};
use rusqlite::{params, OptionalExtension};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

const MAX_LINE: usize = 2 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 12_000;
const MAX_PAGE_CHARS: usize = 120_000;
const ACTIVE_GAP_MS: i64 = 5 * 60_000;

pub fn ensure_schema(db: &Db) -> Result<()> {
    db.lock().execute_batch("CREATE TABLE IF NOT EXISTS session_metadata (
        provider TEXT NOT NULL, session_id TEXT NOT NULL, project TEXT NOT NULL DEFAULT '',
        native_title TEXT, title_priority INTEGER NOT NULL DEFAULT 0, parent_id TEXT,
        first_ts INTEGER, last_ts INTEGER, PRIMARY KEY(provider,session_id));
        CREATE TABLE IF NOT EXISTS session_aliases (
        provider TEXT NOT NULL, session_id TEXT NOT NULL, alias TEXT NOT NULL, PRIMARY KEY(provider,session_id));
        CREATE TABLE IF NOT EXISTS session_sources (
        path TEXT PRIMARY KEY, provider TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
        byte_offset INTEGER NOT NULL, prefix_len INTEGER NOT NULL, prefix_hash TEXT NOT NULL, context_json TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS session_message_refs (
        provider TEXT NOT NULL, session_id TEXT NOT NULL, message_id TEXT NOT NULL,
        path TEXT NOT NULL, byte_offset INTEGER NOT NULL, byte_len INTEGER NOT NULL, line_hash TEXT NOT NULL,
        message_index INTEGER NOT NULL, role TEXT NOT NULL, turn_id TEXT, ts INTEGER, tool_name TEXT,
        is_error INTEGER NOT NULL, is_call INTEGER NOT NULL, call_fingerprint TEXT, content_chars INTEGER NOT NULL,
        PRIMARY KEY(provider,session_id,message_id));
        CREATE INDEX IF NOT EXISTS idx_session_refs_order ON session_message_refs(provider,session_id,ts,byte_offset);
        CREATE INDEX IF NOT EXISTS idx_session_refs_path ON session_message_refs(path);
        CREATE TABLE IF NOT EXISTS session_turns (
        provider TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL,
        started_at INTEGER, finished_at INTEGER, PRIMARY KEY(provider,session_id,turn_id));
        CREATE TABLE IF NOT EXISTS session_usage_links (
        provider TEXT NOT NULL, request_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL,
        PRIMARY KEY(provider,request_id));
        CREATE INDEX IF NOT EXISTS idx_session_usage_turn ON session_usage_links(provider,session_id,turn_id);
        CREATE INDEX IF NOT EXISTS idx_session_parent ON session_metadata(provider,parent_id);")?;
    db.lock().execute_batch("CREATE INDEX IF NOT EXISTS idx_usage_session_lookup ON usage_events(provider,COALESCE(session_id,''),ts);")?;
    Ok(())
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
fn prefix(path: &Path, length: usize) -> Result<String> {
    let mut file = File::open(path)?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    Ok(parser::hash(&bytes))
}
fn tail(path: &Path, end: u64, length: usize) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(end.saturating_sub(length as u64)))?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    Ok(parser::hash(&bytes))
}

/// Consume one physical line with bounded memory. Incomplete trailing writes
/// remain at the previous checkpoint and are retried on the next append.
fn read_line(reader: &mut impl BufRead) -> Result<Option<(Vec<u8>, u64, bool)>> {
    let mut bytes = Vec::new();
    let mut consumed = 0u64;
    let mut overlong = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(if consumed == 0 {
                None
            } else {
                Some((bytes, consumed, false))
            });
        }
        let end = buf.iter().position(|b| *b == b'\n').map(|n| n + 1);
        let take = end.unwrap_or(buf.len());
        if !overlong {
            if bytes.len() + take <= MAX_LINE {
                bytes.extend_from_slice(&buf[..take]);
            } else {
                overlong = true;
                bytes.clear();
            }
        }
        consumed += take as u64;
        reader.consume(take);
        if end.is_some() {
            return Ok(Some((bytes, consumed, true)));
        }
    }
}

pub fn index_codex_titles(db: &Db, path: &Path) -> Result<()> {
    index_file(db, "codex", path)
}

/// Independent, version-stable metadata checkpoint. No prompt, assistant or
/// tool content is written to SQLite, even when content access is enabled.
pub fn index_file(db: &Db, provider: &str, path: &Path) -> Result<()> {
    anyhow::ensure!(
        matches!(provider, "codex" | "claude"),
        "unsupported session provider"
    );
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len() as i64;
    let modified = mtime(&metadata);
    let source = path.to_string_lossy().to_string();
    let previous=db.lock().query_row("SELECT size,mtime,byte_offset,prefix_len,prefix_hash,context_json FROM session_sources WHERE path=?1",
        [&source],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?,row.get::<_,i64>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?))).optional()?;
    let mut offset = 0u64;
    let mut context = Context {
        version: 1,
        file_stem: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session")
            .into(),
        ..Default::default()
    };
    if provider == "claude"
        && path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|p| p.to_str())
            == Some("subagents")
    {
        context.subagent = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.strip_prefix("agent-").unwrap_or(s).to_string());
    }
    let mut reset = false;
    if let Some((old_size, old_mtime, old_offset, prefix_len, prefix_hash, json)) = previous {
        let saved = serde_json::from_str::<Context>(&json)
            .ok()
            .filter(|c| c.version == 1);
        if size == old_size && modified == old_mtime && saved.is_some() {
            return Ok(());
        }
        if size > old_size
            && old_offset >= 0
            && old_offset <= size
            && (0..=4096).contains(&prefix_len)
            && prefix(path, prefix_len as usize)? == prefix_hash
        {
            if let Some(saved) = saved {
                if saved.tail_len <= 4096
                    && saved.tail_len as u64 <= old_offset as u64
                    && (saved.tail_len == 0
                        || tail(path, old_offset as u64, saved.tail_len)? == saved.tail_hash)
                {
                    context = saved;
                    offset = old_offset as u64;
                } else {
                    reset = true;
                }
            } else {
                reset = true;
            }
        } else {
            reset = true;
        }
    }
    if reset {
        db.lock()
            .execute("DELETE FROM session_message_refs WHERE path=?1", [&source])?;
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut done = false;
    while !done {
        let mut batch = Vec::new();
        let mut batch_bytes = 0u64;
        while batch.len() < 100 && batch_bytes < MAX_LINE as u64 {
            let Some((bytes, length, complete)) = read_line(&mut reader)? else {
                done = true;
                break;
            };
            if !complete {
                done = true;
                break;
            }
            let start = offset;
            offset += length;
            batch_bytes += length;
            context.line_no += 1;
            if bytes.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let parsed = parser::parse(provider, &value, &mut context, start);
            if context.session.is_empty() {
                continue;
            }
            let kind = value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if parsed.messages.is_empty()
                && parsed.title.is_none()
                && parsed.usage.is_none()
                && parsed.turn_started.is_none()
                && parsed.turn_finished.is_none()
                && parsed.relink_turn.is_none()
                && !matches!(kind, "session_meta" | "turn_context")
            {
                continue;
            }
            let record_context = Context {
                session: context.session.clone(),
                project: context.project.clone(),
                parent: context.parent.clone(),
                ..Default::default()
            };
            batch.push((record_context, parsed, start, length, parser::hash(&bytes)));
        }
        if batch.is_empty() {
            continue;
        }
        let mut conn = db.lock();
        let tx = conn.transaction()?;
        for (context, parsed, start, length, line_hash) in batch {
            tx.execute("INSERT INTO session_metadata(provider,session_id,project,native_title,title_priority,parent_id,first_ts,last_ts)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?7) ON CONFLICT(provider,session_id) DO UPDATE SET
            project=CASE WHEN excluded.project<>'' THEN excluded.project ELSE project END,
            native_title=CASE WHEN excluded.native_title IS NOT NULL AND excluded.title_priority>=title_priority THEN excluded.native_title ELSE native_title END,
            title_priority=MAX(title_priority,excluded.title_priority),parent_id=COALESCE(excluded.parent_id,parent_id),
            first_ts=CASE WHEN first_ts IS NULL THEN excluded.first_ts WHEN excluded.first_ts IS NULL THEN first_ts ELSE MIN(first_ts,excluded.first_ts) END,
            last_ts=CASE WHEN last_ts IS NULL THEN excluded.last_ts WHEN excluded.last_ts IS NULL THEN last_ts ELSE MAX(last_ts,excluded.last_ts) END",
            params![provider,context.session,context.project,parsed.title.as_ref().map(|t|&t.0),parsed.title.as_ref().map(|t|t.1).unwrap_or(0),context.parent,parsed.timestamp])?;
            for (index, msg) in parsed.messages.iter().enumerate() {
                tx.execute("INSERT INTO session_message_refs(provider,session_id,message_id,path,byte_offset,byte_len,line_hash,message_index,role,turn_id,ts,tool_name,is_error,is_call,call_fingerprint,content_chars)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
                ON CONFLICT(provider,session_id,message_id) DO UPDATE SET
                path=excluded.path,byte_offset=excluded.byte_offset,byte_len=excluded.byte_len,line_hash=excluded.line_hash,message_index=excluded.message_index,
                turn_id=COALESCE(turn_id,excluded.turn_id),ts=COALESCE(excluded.ts,ts),tool_name=COALESCE(excluded.tool_name,tool_name),
                is_error=excluded.is_error,content_chars=excluded.content_chars,call_fingerprint=excluded.call_fingerprint
                WHERE excluded.content_chars>=content_chars",
                params![provider,context.session,msg.id,source,start as i64,length as i64,line_hash,index as i64,msg.role,msg.turn,parsed.timestamp,msg.tool,msg.is_error,msg.is_call,msg.call_fingerprint,msg.text.chars().count() as i64])?;
            }
            if let Some((request, turn)) = &parsed.usage {
                tx.execute("INSERT INTO session_usage_links(provider,request_id,session_id,turn_id) VALUES(?1,?2,?3,?4)
                ON CONFLICT(provider,request_id) DO UPDATE SET turn_id=CASE WHEN session_id=excluded.session_id THEN turn_id ELSE excluded.turn_id END,session_id=excluded.session_id",params![provider,request,context.session,turn])?;
            }
            if let Some((previous, next)) = &parsed.relink_turn {
                tx.execute("UPDATE session_message_refs SET turn_id=?4 WHERE provider=?1 AND session_id=?2 AND turn_id=?3",params![provider,context.session,previous,next])?;
                tx.execute("UPDATE session_usage_links SET turn_id=?4 WHERE provider=?1 AND session_id=?2 AND turn_id=?3",params![provider,context.session,previous,next])?;
                tx.execute("INSERT INTO session_turns(provider,session_id,turn_id,started_at,finished_at)
                SELECT provider,session_id,?4,started_at,finished_at FROM session_turns WHERE provider=?1 AND session_id=?2 AND turn_id=?3
                ON CONFLICT(provider,session_id,turn_id) DO UPDATE SET started_at=COALESCE(started_at,excluded.started_at)",params![provider,context.session,previous,next])?;
                tx.execute(
                    "DELETE FROM session_turns WHERE provider=?1 AND session_id=?2 AND turn_id=?3",
                    params![provider, context.session, previous],
                )?;
            }
            for (turn, started, finished) in [
                parsed
                    .turn_started
                    .as_ref()
                    .map(|t| (t, parsed.timestamp, None)),
                parsed
                    .turn_finished
                    .as_ref()
                    .map(|t| (t, None, parsed.timestamp)),
            ]
            .into_iter()
            .flatten()
            {
                tx.execute("INSERT INTO session_turns(provider,session_id,turn_id,started_at,finished_at) VALUES(?1,?2,?3,?4,?5)
                ON CONFLICT(provider,session_id,turn_id) DO UPDATE SET started_at=COALESCE(started_at,excluded.started_at),finished_at=COALESCE(excluded.finished_at,finished_at)",params![provider,context.session,turn,started,finished])?;
            }
        }
        tx.commit()?;
    }
    // Save metadata from before the read: appends during this pass force a
    // further pass, instead of accidentally skipping unobserved data.
    let prefix_len = (size.max(0) as usize).min(4096);
    context.tail_len = offset.min(4096) as usize;
    context.tail_hash = tail(path, offset, context.tail_len)?;
    db.lock().execute("INSERT INTO session_sources(path,provider,size,mtime,byte_offset,prefix_len,prefix_hash,context_json)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(path) DO UPDATE SET provider=excluded.provider,size=excluded.size,mtime=excluded.mtime,
        byte_offset=excluded.byte_offset,prefix_len=excluded.prefix_len,prefix_hash=excluded.prefix_hash,context_json=excluded.context_json",
        params![source,provider,size,modified,offset as i64,prefix_len as i64,prefix(path,prefix_len)?,serde_json::to_string(&context)?])?;
    Ok(())
}

#[derive(Default)]
struct Range {
    from: Option<i64>,
    to: Option<i64>,
    project: Option<String>,
}
impl Range {
    fn query(q: &SessionListQuery) -> Result<Self> {
        let parse = |v: &Option<String>| -> Result<Option<i64>> {
            v.as_ref()
                .map(|s| parse_time_ms(s).ok_or_else(|| anyhow::anyhow!("invalid session date")))
                .transpose()
        };
        let from = parse(&q.from)?;
        let to = parse(&q.to)?;
        anyhow::ensure!(
            !matches!((from,to),(Some(a),Some(b)) if a>b),
            "session start must precede end"
        );
        Ok(Self {
            from,
            to,
            project: q.project.clone(),
        })
    }
}

pub fn list(
    db: &Db,
    q: &SessionListQuery,
    pricing: &PricingTable,
    content: bool,
) -> Result<SessionListResult> {
    let range = Range::query(q)?;
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(30).clamp(1, 100);
    let sort = match q.sort.as_deref().unwrap_or("recent") {
        "recent" => "last_ts DESC,provider,session_id",
        "tokens" => "tokens DESC,last_ts DESC,provider,session_id",
        "title" => "sort_title COLLATE NOCASE,provider,session_id",
        _ => anyhow::bail!("invalid session sort"),
    };
    // The filtering and LIMIT/OFFSET run in SQLite before loading any detail
    // or transcript. A recent low-token session can never be lost to Top-200.
    let cte="WITH usage AS (SELECT provider,COALESCE(session_id,'') session_id,MAX(ts) last_ts,SUM(total_tokens) tokens,MAX(COALESCE(cwd,'')) project
        FROM usage_events WHERE (?1 IS NULL OR ts>=?1) AND (?2 IS NULL OR ts<?2) AND (?3 IS NULL OR provider=?3) AND (?4 IS NULL OR COALESCE(cwd,'')=?4)
        GROUP BY provider,COALESCE(session_id,'')),
        keys AS (SELECT provider,session_id FROM usage UNION SELECT provider,session_id FROM session_metadata
        WHERE (?1 IS NULL OR last_ts>=?1) AND (?2 IS NULL OR first_ts<?2) AND (?3 IS NULL OR provider=?3) AND (?4 IS NULL OR project=?4)),
        candidates AS (SELECT k.provider,k.session_id,CASE WHEN ?1 IS NULL AND ?2 IS NULL THEN MAX(COALESCE(u.last_ts,0),COALESCE(m.last_ts,0)) ELSE COALESCE(u.last_ts,m.last_ts,0) END last_ts,COALESCE(u.tokens,0) tokens,
        COALESCE(a.alias,m.native_title,NULLIF(m.project,''),NULLIF(u.project,''),k.session_id) sort_title,
        COALESCE(NULLIF(m.project,''),u.project,'') project FROM keys k LEFT JOIN usage u USING(provider,session_id)
        LEFT JOIN session_metadata m USING(provider,session_id) LEFT JOIN session_aliases a USING(provider,session_id)),
        filtered AS (SELECT * FROM candidates WHERE ?5='' OR instr(lower(sort_title||' '||session_id||' '||project),lower(?5))>0) ";
    let search = q.search.as_deref().unwrap_or("").trim();
    anyhow::ensure!(search.len() <= 512, "session search is too long");
    let params = params![
        range.from,
        range.to,
        q.provider,
        range.project,
        search,
        limit,
        offset
    ];
    let (total, keys) = {
        let conn = db.lock();
        let total = conn.query_row(
            &format!("{cte} SELECT COUNT(*) FROM filtered WHERE ?6>0 AND ?7>=0"),
            params,
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(&format!(
            "{cte} SELECT provider,session_id FROM filtered ORDER BY {sort} LIMIT ?6 OFFSET ?7"
        ))?;
        let keys = stmt
            .query_map(params, |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        (total, keys)
    };
    let mut rows = Vec::with_capacity(keys.len());
    for (provider, session) in keys {
        rows.push(summary(db, &provider, &session, pricing, content, &range)?);
    }
    Ok(SessionListResult {
        rows,
        total,
        offset,
        limit,
    })
}

fn add_totals(target: &mut TokenTotals, value: &TokenTotals) {
    target.input_tokens += value.input_tokens;
    target.cache_write_tokens += value.cache_write_tokens;
    target.cache_read_tokens += value.cache_read_tokens;
    target.output_tokens += value.output_tokens;
    target.reasoning_tokens += value.reasoning_tokens;
    target.total_tokens += value.total_tokens;
    target.requests += value.requests;
    if let Some(cost) = value.estimated_cost_usd {
        target.known_cost_usd = Some(target.known_cost_usd.unwrap_or(0.0) + cost);
    } else {
        target.unpriced_requests += value.requests;
    }
    target.estimated_cost_usd = if target.unpriced_requests == 0 {
        target.known_cost_usd
    } else {
        None
    };
}

type UsageAggregate = (
    TokenTotals,
    Vec<ModelVariant>,
    Option<i64>,
    Option<i64>,
    String,
);

fn usage(
    db: &Db,
    provider: &str,
    session: &str,
    turn: Option<&str>,
    range: &Range,
    prices: &PricingTable,
) -> Result<UsageAggregate> {
    let conn = db.lock();
    let mut stmt=conn.prepare("SELECT u.model,u.reasoning_effort,SUM(u.input_tokens),SUM(u.cache_write_tokens),SUM(u.cache_read_tokens),
        SUM(u.output_tokens),SUM(u.reasoning_tokens),SUM(u.total_tokens),COUNT(*),MIN(u.ts),MAX(u.ts)
        FROM usage_events u WHERE u.provider=?1 AND COALESCE(u.session_id,'')=?2 AND (?3 IS NULL OR u.ts>=?3) AND (?4 IS NULL OR u.ts<?4)
        AND (?5 IS NULL OR COALESCE(u.cwd,'')=?5) AND (?6 IS NULL OR EXISTS(SELECT 1 FROM session_usage_links l WHERE l.provider=u.provider AND l.request_id=u.request_id AND l.turn_id=?6 AND l.session_id=?2))
        GROUP BY u.model,u.reasoning_effort ORDER BY u.model,u.reasoning_effort")?;
    let mut rows = stmt.query(params![
        provider,
        session,
        range.from,
        range.to,
        range.project,
        turn
    ])?;
    let mut totals = TokenTotals::default();
    let mut variants = Vec::new();
    let mut first = None;
    let mut last = None;
    while let Some(row) = rows.next()? {
        let model: String = row.get(0)?;
        let mut value = TokenTotals {
            input_tokens: row.get(2)?,
            cache_write_tokens: row.get(3)?,
            cache_read_tokens: row.get(4)?,
            output_tokens: row.get(5)?,
            reasoning_tokens: row.get(6)?,
            total_tokens: row.get(7)?,
            requests: row.get(8)?,
            ..Default::default()
        };
        value.estimated_cost_usd = pricing::estimate_cost(prices, &model, &value);
        add_totals(&mut totals, &value);
        variants.push(ModelVariant {
            model,
            reasoning_effort: row.get(1)?,
        });
        let a: i64 = row.get(9)?;
        let b: i64 = row.get(10)?;
        first = Some(first.map_or(a, |v: i64| v.min(a)));
        last = Some(last.map_or(b, |v: i64| v.max(b)));
    }
    let project=conn.query_row("SELECT COALESCE(cwd,'') FROM usage_events WHERE provider=?1 AND COALESCE(session_id,'')=?2
        AND (?3 IS NULL OR ts>=?3) AND (?4 IS NULL OR ts<?4) AND (?5 IS NULL OR COALESCE(cwd,'')=?5) ORDER BY ts DESC,id DESC LIMIT 1",params![provider,session,range.from,range.to,range.project],|r|r.get(0)).optional()?.unwrap_or_default();
    Ok((totals, variants, first, last, project))
}

fn summary(
    db: &Db,
    provider: &str,
    session: &str,
    prices: &PricingTable,
    content: bool,
    range: &Range,
) -> Result<SessionSummary> {
    let (totals, variants, first, last, usage_project) =
        usage(db, provider, session, None, range, prices)?;
    let metadata=db.lock().query_row("SELECT m.project,m.native_title,m.parent_id,m.first_ts,m.last_ts,a.alias FROM session_metadata m
        LEFT JOIN session_aliases a USING(provider,session_id) WHERE m.provider=?1 AND m.session_id=?2",params![provider,session],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<i64>>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,Option<String>>(5)?))).optional()?;
    let (meta_project, native, parent, meta_first, meta_last, mut alias) =
        metadata.unwrap_or_default();
    let parent = parent.or_else(|| {
        session
            .rsplit_once(":agent:")
            .map(|(parent, _)| parent.to_string())
    });
    if alias.is_none() {
        alias = db
            .lock()
            .query_row(
                "SELECT alias FROM session_aliases WHERE provider=?1 AND session_id=?2",
                params![provider, session],
                |r| r.get(0),
            )
            .optional()?;
    }
    let project = if usage_project.is_empty() {
        meta_project
    } else {
        usage_project
    };
    let outside_filter = range.from.is_none() && range.to.is_none() && range.project.is_none();
    let first = if outside_filter {
        first.into_iter().chain(meta_first).min()
    } else {
        first.or(meta_first)
    }
    .unwrap_or(0);
    let last = if outside_filter {
        last.into_iter().chain(meta_last).max()
    } else {
        last.or(meta_last)
    }
    .unwrap_or(first);
    let (users,calls,failures,repeats)=db.lock().query_row("SELECT COUNT(DISTINCT CASE WHEN role='user' THEN COALESCE(turn_id,message_id) END),
        COALESCE(SUM(is_call),0),COALESCE(SUM(is_error),0),COALESCE(SUM(is_call),0)-COUNT(DISTINCT CASE WHEN is_call=1 THEN call_fingerprint END)
        FROM session_message_refs WHERE provider=?1 AND session_id=?2 AND (?3 IS NULL OR ts>=?3) AND (?4 IS NULL OR ts<?4)",params![provider,session,range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,i64>(3)?.max(0))))?;
    let active:Option<i64>=db.lock().query_row("SELECT SUM(MIN(ts-previous,?5)) FROM (SELECT ts,LAG(ts) OVER (ORDER BY ts) previous FROM session_message_refs
        WHERE provider=?1 AND session_id=?2 AND ts IS NOT NULL AND (?3 IS NULL OR ts>=?3) AND (?4 IS NULL OR ts<?4)) WHERE previous IS NOT NULL",params![provider,session,range.from,range.to,ACTIVE_GAP_MS],|r|r.get(0))?;
    let paths = {
        let conn = db.lock();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT path FROM session_message_refs WHERE provider=?1 AND session_id=?2",
        )?;
        let values = stmt
            .query_map(params![provider, session], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        values
    };
    let available = paths.iter().any(|p| Path::new(p).is_file());
    let (mut title, mut title_source) = if let Some(alias) = alias {
        (alias, "alias")
    } else if let Some(native) = native {
        (native, "native")
    } else {
        (fallback_title(&project, session, first), "fallback")
    };
    if content && title_source == "fallback" {
        if let Ok((messages, _, _, _)) = messages(db, provider, session, 0, 1, Some("user")) {
            if let Some(first) = messages.first() {
                title = first
                    .text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(90)
                    .collect();
                title_source = "prompt";
            }
        }
    }
    let models = variants
        .iter()
        .map(|v| v.model.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(SessionSummary {
        usage: SessionRow {
            session_id: session.into(),
            provider: provider.into(),
            project,
            first_ts: local_rfc3339(first),
            last_ts: local_rfc3339(last),
            duration_ms: last.saturating_sub(first),
            models,
            model_variants: variants,
            totals,
        },
        title,
        title_source: title_source.into(),
        parent_session_id: parent,
        user_turns: users,
        tool_calls: calls,
        tool_failures: failures,
        repeated_tool_calls: repeats,
        active_duration_ms: active,
        transcript_available: available,
    })
}

fn fallback_title(project: &str, session: &str, first: i64) -> String {
    let project = project
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or("");
    if session.is_empty() {
        return "Unassigned session".into();
    }
    if project.is_empty() {
        format!("Session {}", session.chars().take(12).collect::<String>())
    } else {
        format!(
            "{project} · {}",
            local_rfc3339(first).chars().take(10).collect::<String>()
        )
    }
}

struct Reference {
    id: String,
    path: String,
    offset: u64,
    length: u64,
    hash: String,
    index: usize,
    role: String,
    turn: Option<String>,
    timestamp: Option<i64>,
    tool: Option<String>,
    is_error: bool,
}

fn unsigned_column(row: &rusqlite::Row<'_>, index: usize, max: u64) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    let unsigned =
        u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))?;
    if unsigned > max {
        return Err(rusqlite::Error::IntegralValueOutOfRange(index, value));
    }
    Ok(unsigned)
}

fn messages(
    db: &Db,
    provider: &str,
    session: &str,
    offset: u32,
    limit: u32,
    role: Option<&str>,
) -> Result<(Vec<SessionMessage>, i64, Vec<String>, u32)> {
    let (refs, total) = {
        let conn = db.lock();
        let total=conn.query_row("SELECT COUNT(*) FROM session_message_refs WHERE provider=?1 AND session_id=?2 AND (?3 IS NULL OR role=?3)",params![provider,session,role],|r|r.get(0))?;
        let mut stmt=conn.prepare("SELECT message_id,path,byte_offset,byte_len,line_hash,message_index,role,turn_id,ts,tool_name,is_error
            FROM session_message_refs WHERE provider=?1 AND session_id=?2 AND (?3 IS NULL OR role=?3) ORDER BY COALESCE(ts,0),path,byte_offset,message_index LIMIT ?4 OFFSET ?5")?;
        let refs = stmt
            .query_map(params![provider, session, role, limit, offset], |r| {
                Ok(Reference {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    offset: unsigned_column(r, 2, i64::MAX as u64)?,
                    length: unsigned_column(r, 3, MAX_LINE as u64)?,
                    hash: r.get(4)?,
                    index: unsigned_column(r, 5, MAX_LINE as u64)? as usize,
                    role: r.get(6)?,
                    turn: r.get(7)?,
                    timestamp: r.get(8)?,
                    tool: r.get(9)?,
                    is_error: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        (refs, total)
    };
    let mut messages = Vec::new();
    let mut warnings = BTreeSet::new();
    let mut remaining = MAX_PAGE_CHARS;
    let mut consumed = 0;
    for reference in refs {
        let read = || -> Result<SessionMessage> {
            anyhow::ensure!(
                reference.length <= MAX_LINE as u64,
                "message exceeds the local read limit"
            );
            let mut file =
                File::open(&reference.path).context("source log is unavailable or was removed")?;
            anyhow::ensure!(
                reference
                    .offset
                    .checked_add(reference.length)
                    .is_some_and(|end| file.metadata().is_ok_and(|m| m.len() >= end)),
                "source log was replaced or truncated; reindex is required"
            );
            file.seek(SeekFrom::Start(reference.offset))?;
            let mut bytes = vec![0; reference.length as usize];
            file.read_exact(&mut bytes)?;
            anyhow::ensure!(
                parser::hash(&bytes) == reference.hash,
                "source log was changed; reindex is required"
            );
            let value = serde_json::from_slice(&bytes)?;
            let mut context = Context {
                session: session.into(),
                turn: reference.turn.clone(),
                ..Default::default()
            };
            let parsed = parser::parse(provider, &value, &mut context, reference.offset);
            let message = parsed.messages.get(reference.index).ok_or_else(|| {
                anyhow::anyhow!("message is no longer supported by the source adapter")
            })?;
            let max = MAX_MESSAGE_CHARS;
            let text = message.text.chars().take(max).collect::<String>();
            let truncated = message.text.chars().count() > max;
            Ok(SessionMessage {
                id: reference.id.clone(),
                turn_id: reference.turn.clone(),
                role: reference.role.clone(),
                text,
                timestamp: reference.timestamp,
                tool_name: reference.tool.clone(),
                is_error: reference.is_error,
                truncated,
            })
        };
        match read() {
            Ok(message) => {
                let chars = message.text.chars().count();
                if chars > remaining {
                    break;
                }
                remaining -= chars;
                messages.push(message);
            }
            Err(error) => {
                warnings.insert(format!("Transcript unavailable: {error}"));
            }
        }
        consumed += 1;
    }
    Ok((messages, total, warnings.into_iter().collect(), consumed))
}

pub fn content_page(
    db: &Db,
    provider: &str,
    session: &str,
    offset: u32,
    limit: u32,
    content: bool,
) -> Result<SessionContentPage> {
    anyhow::ensure!(content, "local session content access is disabled");
    let (messages, total, warnings, consumed) =
        messages(db, provider, session, offset, limit.clamp(1, 100), None)?;
    let updated:Option<i64>=db.lock().query_row("SELECT MAX(s.mtime/1000000) FROM session_sources s JOIN
        (SELECT DISTINCT path FROM session_message_refs WHERE provider=?1 AND session_id=?2) r ON r.path=s.path",params![provider,session],|r|r.get(0))?;
    let next = offset.saturating_add(consumed);
    Ok(SessionContentPage {
        messages,
        total_messages: total,
        next_offset: if i64::from(next) < total {
            Some(next)
        } else {
            None
        },
        warnings,
        source_updated_at: updated.unwrap_or(0),
    })
}

pub fn detail(
    db: &Db,
    provider: &str,
    session: &str,
    offset: u32,
    limit: u32,
    prices: &PricingTable,
    content: bool,
) -> Result<SessionDetail> {
    let exists:bool=db.lock().query_row("SELECT EXISTS(SELECT 1 FROM session_metadata WHERE provider=?1 AND session_id=?2 UNION ALL SELECT 1 FROM usage_events WHERE provider=?1 AND COALESCE(session_id,'')=?2)",params![provider,session],|r|r.get(0))?;
    anyhow::ensure!(exists, "session was not found");
    let limit = limit.clamp(1, 100);
    let summary = summary(db, provider, session, prices, content, &Range::default())?;
    let (messages, total_messages, mut warnings, consumed) = if content {
        messages(db, provider, session, offset, limit, None)?
    } else {
        let total = db.lock().query_row(
            "SELECT COUNT(*) FROM session_message_refs WHERE provider=?1 AND session_id=?2",
            params![provider, session],
            |r| r.get(0),
        )?;
        (
            vec![],
            total,
            vec![
                "Transcript access is disabled. Enable local content access to display messages."
                    .into(),
            ],
            0,
        )
    };
    let turns = turns(db, provider, session, prices)?;
    if turns.len() == 500 {
        warnings.push("Turn details are limited to the first 500 observed turns.".into());
    }
    if !summary.transcript_available {
        warnings.push("Original session logs are missing or no supported messages were indexed. Token history remains available.".into());
    }
    warnings.push("Activity span includes idle time. Estimated active time caps each observed message gap at five minutes; it is not generation speed or working time. Turn usage is unknown when no explicit linkage was observed.".into());
    let child_ids = {
        let conn = db.lock();
        let mut stmt=conn.prepare("SELECT session_id FROM (
            SELECT session_id,last_ts FROM session_metadata WHERE provider=?1 AND parent_id=?2
            UNION SELECT session_id,MAX(ts) FROM usage_events WHERE provider=?1 AND substr(session_id,1,length(?2)+7)=?2||':agent:' GROUP BY session_id)
            GROUP BY session_id ORDER BY MAX(last_ts) DESC LIMIT 100")?;
        let ids = stmt
            .query_map(params![provider, session], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };
    let mut children = Vec::new();
    for child in child_ids {
        children.push(self::summary(
            db,
            provider,
            &child,
            prices,
            content,
            &Range::default(),
        )?);
    }
    let next = offset.saturating_add(consumed);
    let source_updated_at:Option<i64>=db.lock().query_row("SELECT MAX(s.mtime/1000000) FROM session_sources s JOIN
        (SELECT DISTINCT path FROM session_message_refs WHERE provider=?1 AND session_id=?2) r ON r.path=s.path",params![provider,session],|r|r.get(0))?;
    Ok(SessionDetail {
        source_updated_at,
        session: summary,
        messages,
        total_messages,
        next_offset: if content && i64::from(next) < total_messages {
            Some(next)
        } else {
            None
        },
        turns,
        children,
        warnings,
    })
}

fn turns(
    db: &Db,
    provider: &str,
    session: &str,
    prices: &PricingTable,
) -> Result<Vec<SessionTurn>> {
    let rows = {
        let conn = db.lock();
        let mut stmt=conn.prepare("WITH keys AS (SELECT turn_id FROM session_turns WHERE provider=?1 AND session_id=?2
        UNION SELECT turn_id FROM session_message_refs WHERE provider=?1 AND session_id=?2 AND turn_id IS NOT NULL
        UNION SELECT turn_id FROM session_usage_links WHERE provider=?1 AND session_id=?2)
        SELECT k.turn_id,COALESCE(t.started_at,MIN(r.ts)),COALESCE(t.finished_at,MAX(r.ts)),MIN(CASE WHEN r.role='user' THEN r.message_id END),
        COALESCE(SUM(r.is_call),0),COALESCE(SUM(r.is_error),0) FROM keys k LEFT JOIN session_turns t ON t.provider=?1 AND t.session_id=?2 AND t.turn_id=k.turn_id
        LEFT JOIN session_message_refs r ON r.provider=?1 AND r.session_id=?2 AND r.turn_id=k.turn_id
        GROUP BY k.turn_id ORDER BY COALESCE(t.started_at,MIN(r.ts)),k.turn_id LIMIT 500")?;
        let rows = stmt
            .query_map(params![provider, session], |r| {
                Ok(SessionTurn {
                    id: r.get(0)?,
                    started_at: r.get(1)?,
                    finished_at: r.get(2)?,
                    user_message_id: r.get(3)?,
                    totals: None,
                    tool_calls: r.get(4)?,
                    tool_failures: r.get(5)?,
                    duration_ms: None,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    // Price once per turn/model aggregate, instead of rescanning the same
    // session once for each of its (up to 500) turns.
    let mut per_turn: BTreeMap<String, TokenTotals> = BTreeMap::new();
    {
        let conn = db.lock();
        let mut stmt=conn.prepare("SELECT l.turn_id,u.model,SUM(u.input_tokens),SUM(u.cache_write_tokens),SUM(u.cache_read_tokens),
            SUM(u.output_tokens),SUM(u.reasoning_tokens),SUM(u.total_tokens),COUNT(*)
            FROM session_usage_links l JOIN usage_events u ON u.provider=l.provider AND u.request_id=l.request_id
            WHERE l.provider=?1 AND l.session_id=?2 GROUP BY l.turn_id,u.model")?;
        let mut records = stmt.query(params![provider, session])?;
        while let Some(row) = records.next()? {
            let id: String = row.get(0)?;
            let model: String = row.get(1)?;
            let mut value = TokenTotals {
                input_tokens: row.get(2)?,
                cache_write_tokens: row.get(3)?,
                cache_read_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                reasoning_tokens: row.get(6)?,
                total_tokens: row.get(7)?,
                requests: row.get(8)?,
                ..Default::default()
            };
            value.estimated_cost_usd = pricing::estimate_cost(prices, &model, &value);
            add_totals(per_turn.entry(id).or_default(), &value);
        }
    }
    let mut result = Vec::new();
    for mut turn in rows {
        turn.totals = per_turn.remove(&turn.id);
        turn.duration_ms = turn
            .started_at
            .zip(turn.finished_at)
            .map(|(start, end)| end.saturating_sub(start).max(0));
        result.push(turn);
    }
    Ok(result)
}

pub fn set_alias(db: &Db, provider: &str, session: &str, alias: &str) -> Result<()> {
    let alias = alias.trim();
    anyhow::ensure!(
        alias.chars().count() <= 200,
        "session alias must be at most 200 characters"
    );
    anyhow::ensure!(
        matches!(provider, "claude" | "codex"),
        "unsupported session provider"
    );
    if alias.is_empty() {
        db.lock().execute(
            "DELETE FROM session_aliases WHERE provider=?1 AND session_id=?2",
            params![provider, session],
        )?;
    } else {
        db.lock().execute(
            "INSERT INTO session_aliases(provider,session_id,alias) VALUES(?1,?2,?3)
        ON CONFLICT(provider,session_id) DO UPDATE SET alias=excluded.alias",
            params![provider, session, alias],
        )?;
    }
    Ok(())
}
pub fn clear_analysis(db: &Db, provider: &str, session: &str) -> Result<()> {
    db.lock().execute(
        "DELETE FROM session_aliases WHERE provider=?1 AND session_id=?2",
        params![provider, session],
    )?;
    Ok(())
}
