-- Database as written by v0.6.0 (schema_version 3: quota_samples.account, idx_quota_account_ts, no idx_quota_ts).
-- Source: git show v0.6.0:src-tauri/src/store/mod.rs migrate() (v3 step) on top of the v0.5.0 layout.
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
CREATE TABLE ingest_checkpoints (
  path TEXT PRIMARY KEY,
  checkpoint TEXT NOT NULL);
CREATE TABLE ingest_legacy_events (
  path TEXT NOT NULL,
  request_id TEXT NOT NULL,
  PRIMARY KEY(path, request_id));
CREATE INDEX idx_usage_source ON usage_events(provider, source_file);
ALTER TABLE usage_events ADD COLUMN reasoning_effort TEXT;
CREATE TABLE session_metadata (
  provider TEXT NOT NULL, session_id TEXT NOT NULL, project TEXT NOT NULL DEFAULT '',
  native_title TEXT, title_priority INTEGER NOT NULL DEFAULT 0, parent_id TEXT,
  first_ts INTEGER, last_ts INTEGER, PRIMARY KEY(provider,session_id));
CREATE TABLE session_aliases (
  provider TEXT NOT NULL, session_id TEXT NOT NULL, alias TEXT NOT NULL, PRIMARY KEY(provider,session_id));
CREATE TABLE session_sources (
  path TEXT PRIMARY KEY, provider TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  byte_offset INTEGER NOT NULL, prefix_len INTEGER NOT NULL, prefix_hash TEXT NOT NULL, context_json TEXT NOT NULL);
CREATE TABLE session_message_refs (
  provider TEXT NOT NULL, session_id TEXT NOT NULL, message_id TEXT NOT NULL,
  path TEXT NOT NULL, byte_offset INTEGER NOT NULL, byte_len INTEGER NOT NULL, line_hash TEXT NOT NULL,
  message_index INTEGER NOT NULL, role TEXT NOT NULL, turn_id TEXT, ts INTEGER, tool_name TEXT,
  is_error INTEGER NOT NULL, is_call INTEGER NOT NULL, call_fingerprint TEXT, content_chars INTEGER NOT NULL,
  PRIMARY KEY(provider,session_id,message_id));
CREATE INDEX idx_session_refs_order ON session_message_refs(provider,session_id,ts,byte_offset);
CREATE INDEX idx_session_refs_path ON session_message_refs(path);
CREATE TABLE session_turns (
  provider TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL,
  started_at INTEGER, finished_at INTEGER, PRIMARY KEY(provider,session_id,turn_id));
CREATE TABLE session_usage_links (
  provider TEXT NOT NULL, request_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL,
  PRIMARY KEY(provider,request_id));
CREATE INDEX idx_session_usage_turn ON session_usage_links(provider,session_id,turn_id);
CREATE INDEX idx_session_parent ON session_metadata(provider,parent_id);
CREATE INDEX idx_usage_session_lookup ON usage_events(provider,COALESCE(session_id,''),ts);
CREATE TABLE session_evaluations (
  id TEXT PRIMARY KEY, provider TEXT NOT NULL, session_id TEXT NOT NULL,
  created_at INTEGER NOT NULL, cache_key TEXT NOT NULL,
  report_json TEXT NOT NULL, original_report_json TEXT NOT NULL);
CREATE INDEX idx_evaluation_session ON session_evaluations(provider,session_id,created_at);
ALTER TABLE quota_samples ADD COLUMN account TEXT NOT NULL DEFAULT '';
DROP INDEX idx_quota_ts;
CREATE INDEX idx_quota_account_ts ON quota_samples(provider, account, ts);
INSERT INTO usage_events(id,provider,model,ts,input_tokens,cache_write_tokens,cache_read_tokens,output_tokens,reasoning_tokens,total_tokens,session_id,request_id,cwd,source_file) VALUES
 (1,'claude','claude-sonnet-4-5',1759980000000,1200,300,5000,800,0,7300,'sess-a','req-a1','/home/u/proj-a','/home/u/.claude/projects/proj-a/sess-a.jsonl'),
 (2,'claude','claude-opus-4-1',1759981000000,400,0,9000,1500,0,10900,'sess-a','req-a2','/home/u/proj-a','/home/u/.claude/projects/proj-a/sess-a.jsonl'),
 (3,'codex','gpt-5',1759982000000,2500,0,1000,600,200,4300,'sess-c','req-c1','/home/u/proj-b','/home/u/.codex/sessions/2025/10/09/rollout-c.jsonl'),
 (4,'codex','gpt-5-codex',1759983000000,900,0,0,300,100,1300,'sess-c','req-c2','/home/u/proj-b','/home/u/.codex/sessions/2025/10/09/rollout-c.jsonl');
UPDATE usage_events SET reasoning_effort='high' WHERE id=3;
INSERT INTO quota_samples(id,provider,kind,scope,used_percent,resets_at,plan,ts) VALUES
 (1,'claude','five_hour',NULL,12.5,1760000000000,'max',1759990000000),
 (2,'claude','seven_day',NULL,40.0,1760500000000,'max',1759990000000),
 (3,'claude','seven_day','sonnet',22.0,1760500000000,'max',1759990000000),
 (4,'codex','five_hour',NULL,61.0,1760003000000,'plus',1759990060000),
 (5,'codex','seven_day',NULL,8.0,1760600000000,'plus',1759990060000);
INSERT INTO ingest_files(path,provider,size,mtime,byte_offset,last_ingested_at) VALUES
 ('/home/u/.claude/projects/proj-a/sess-a.jsonl','claude',20480,1759981000000,20480,1759981100000),
 ('/home/u/.codex/sessions/2025/10/09/rollout-c.jsonl','codex',8192,1759983000000,8192,1759983100000);
UPDATE usage_events SET reasoning_effort='high' WHERE id=3;
INSERT INTO usage_events(id,provider,model,ts,input_tokens,output_tokens,total_tokens,session_id,request_id,cwd,source_file) VALUES (5,'claude','claude-sonnet-4-5',1759984000000,100,50,150,'sess-a:agent:abc','req-sub1','/home/u/proj-a','/home/u/.claude/projects/proj-a/sess-a/subagents/agent-abc.jsonl');
INSERT INTO session_metadata(provider,session_id,project,native_title,title_priority,parent_id,first_ts,last_ts) VALUES
 ('claude','sess-a','proj-a','Refactor parser',1,NULL,1759980000000,1759981000000),
 ('codex','sess-c','proj-b',NULL,0,NULL,1759982000000,1759983000000);
INSERT INTO session_aliases(provider,session_id,alias) VALUES ('claude','sess-a','my refactor');
INSERT INTO session_sources(path,provider,size,mtime,byte_offset,prefix_len,prefix_hash,context_json) VALUES ('/home/u/.claude/projects/proj-a/sess-a.jsonl','claude',20480,1759981000000,20480,64,'abc123','{}');
INSERT INTO session_turns(provider,session_id,turn_id,started_at,finished_at) VALUES ('claude','sess-a','t1',1759980000000,1759980500000);
INSERT INTO session_usage_links(provider,request_id,session_id,turn_id) VALUES ('claude','req-a1','sess-a','t1');
INSERT INTO session_evaluations(id,provider,session_id,created_at,cache_key,report_json,original_report_json) VALUES ('ev1','claude','sess-a',1759985000000,'k1','{"score":80}','{"score":80}');
INSERT INTO ingest_checkpoints(path,checkpoint) VALUES ('/home/u/.codex/sessions/2025/10/09/rollout-c.jsonl','{"total":1300}');
UPDATE quota_samples SET account='work' WHERE id=3;
INSERT INTO meta(key,value) VALUES('schema_version','3'),('claude_child_attribution_v1','1');
