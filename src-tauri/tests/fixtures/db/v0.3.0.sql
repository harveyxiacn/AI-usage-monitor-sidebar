-- Database as written by v0.3.0 / v0.4.0 / v0.4.1 / v0.4.2 (schema_version 2).
-- Source: git show v0.3.0:src-tauri/src/store/mod.rs, migrate() = v0.2.2 tables + ALTER TABLE usage_events ADD COLUMN reasoning_effort TEXT (identical in v0.4.x).
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE usage_events (
  id INTEGER PRIMARY KEY,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  ts INTEGER NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  cache_write_tokens INTEGER NOT NULL DEFAULT 0,
  cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  reasoning_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  session_id TEXT,
  request_id TEXT NOT NULL,
  cwd TEXT,
  source_file TEXT,
  UNIQUE(provider, request_id));
CREATE INDEX idx_usage_ts ON usage_events(ts);
CREATE TABLE quota_samples (
  id INTEGER PRIMARY KEY,
  provider TEXT NOT NULL,
  kind TEXT NOT NULL,
  scope TEXT,
  used_percent REAL NOT NULL,
  resets_at INTEGER,
  plan TEXT,
  ts INTEGER NOT NULL);
CREATE INDEX idx_quota_ts ON quota_samples(provider, ts);
CREATE TABLE ingest_files (
  path TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL,
  byte_offset INTEGER NOT NULL,
  last_ingested_at INTEGER NOT NULL);
ALTER TABLE usage_events ADD COLUMN reasoning_effort TEXT;
INSERT INTO usage_events(id,provider,model,ts,input_tokens,cache_write_tokens,cache_read_tokens,output_tokens,reasoning_tokens,total_tokens,session_id,request_id,cwd,source_file) VALUES
 (1,'claude','claude-sonnet-4-5',1759980000000,1200,300,5000,800,0,7300,'sess-a','req-a1','/home/u/proj-a','/home/u/.claude/projects/proj-a/sess-a.jsonl'),
 (2,'claude','claude-opus-4-1',1759981000000,400,0,9000,1500,0,10900,'sess-a','req-a2','/home/u/proj-a','/home/u/.claude/projects/proj-a/sess-a.jsonl'),
 (3,'codex','gpt-5',1759982000000,2500,0,1000,600,200,4300,'sess-c','req-c1','/home/u/proj-b','/home/u/.codex/sessions/2025/10/09/rollout-c.jsonl'),
 (4,'codex','gpt-5-codex',1759983000000,900,0,0,300,100,1300,'sess-c','req-c2','/home/u/proj-b','/home/u/.codex/sessions/2025/10/09/rollout-c.jsonl');
UPDATE usage_events SET reasoning_effort='high' WHERE id=3;
INSERT INTO usage_events(id,provider,model,ts,input_tokens,output_tokens,total_tokens,session_id,request_id,cwd,source_file) VALUES (5,'claude','claude-sonnet-4-5',1759984000000,100,50,150,'sess-a','req-sub1','/home/u/proj-a','/home/u/.claude/projects/proj-a/sess-a/subagents/agent-abc.jsonl');
INSERT INTO quota_samples(id,provider,kind,scope,used_percent,resets_at,plan,ts) VALUES
 (1,'claude','five_hour',NULL,12.5,1760000000000,'max',1759990000000),
 (2,'claude','seven_day',NULL,40.0,1760500000000,'max',1759990000000),
 (3,'claude','seven_day','sonnet',22.0,1760500000000,'max',1759990000000),
 (4,'codex','five_hour',NULL,61.0,1760003000000,'plus',1759990060000),
 (5,'codex','seven_day',NULL,8.0,1760600000000,'plus',1759990060000);
INSERT INTO ingest_files(path,provider,size,mtime,byte_offset,last_ingested_at) VALUES
 ('/home/u/.claude/projects/proj-a/sess-a.jsonl','claude',20480,1759981000000,20480,1759981100000),
 ('/home/u/.codex/sessions/2025/10/09/rollout-c.jsonl','codex',8192,1759983000000,8192,1759983100000);
INSERT INTO meta(key,value) VALUES('schema_version','2');
