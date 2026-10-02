# Performance validation

All fixtures in the commands below are generated locally from synthetic data. They do not read CLI authentication or personal session logs.

## Backend benchmarks

From `src-tauri`:

```sh
cargo test --locked synthetic_ingestion_benchmark --lib -- --ignored --nocapture
cargo test --locked synthetic_query_benchmark --lib -- --ignored --nocapture
```

The ingestion benchmark creates a roughly 20 MB Codex file, indexes it once, then appends 20 small usage records. It compares the previous whole-file parsing path with normal incremental ingestion. The baseline timing includes parsing; the new timing additionally includes metadata indexing and SQLite writes, so these are deliberately different work scopes. `baseline_usage_bytes` and `checkpoint_usage_bytes` count the usage-parser input. The latter does not count session-index reads; `anchor_bytes_max` reports the upper bound for usage checkpoint validation/saving. Cache warmth and debug builds affect timings; retain the printed values and build profile when reporting results.

The query benchmark loads 50,000 usage events, runs the same history query 20 times without the cache, warms the cache, and runs it 20 more times. It reports elapsed time and the number of event rows scanned. The warm path performs no event scan. It still deserializes the bounded aggregate result and performs normal query/pricing key validation.

These are repeatable measurements, not startup or frame-rate measurements. Application launch, initial indexing and UI rendering must be measured separately.

## Correctness gates

```sh
cargo test --locked commands::ingest --lib
cargo test --locked commands::store --lib
cargo test --locked dirty_queue --lib
```

Fixtures cover restart with saved context, delayed named turns, partial final lines, truncation and common growing rewrites, modern records superseding legacy fallback counts, skipped oversized lines, child-session attribution, metadata-enrichment cache invalidation, repricing, filtering, queue debounce, and queue overflow. An analysis-index failure must not prevent existing token-history ingestion.

## Bounds and semantics

- Usage ingestion reads about 4 MiB per chunk, extending through a complete line. A single line may retain at most 8 MiB; larger lines are skipped after their newline. Complete-line offsets and physical line numbers remain stable. An oversized line that has not yet ended may be scanned again after an append; memory remains bounded.
- Codex checkpoints retain model/project/effort context, per-turn context, cumulative counters, usage format and physical line position. They contain no prompt, assistant or tool text. The retained turn-context map grows with the number of turns in a file so delayed responses can retain their original metadata.
- Prefix and suffix anchors plus file metadata detect common replacements without rereading all consumed bytes. An in-place modification exclusively in the middle, with unchanged anchor bytes and preserved file metadata, cannot be detected by this fast path. A manual full reindex rebuilds parsing state.
- Modern `token_usage_record` records take precedence over cumulative `token_count` fallback records from that source, preserving the previous whole-file parser's counting contract. A source that genuinely changes CLI format without replaying older requests is intrinsically ambiguous; the parser does not invent overlap or duplicate counters. Known modern responses retain provider request-ID deduplication.
- Aggregate caches include every query field, pricing data, a usage revision and timezone information. They expire after 30 seconds and retain at most 16 entries / 8 MiB. Inserts, streaming counter upgrades and metadata correction invalidate the cache.
- File notifications deduplicate up to 4,096 changed paths. A batch runs after three quiet seconds or ten seconds of continuous writing. Queue overflow and periodic reconciliation scan the directories, so missed watcher notifications do not permanently lose usage.
- Claude child files preserve separate own totals. A one-time migration corrects stored usage whose source is explicitly under a `subagents` directory, even if the source file has expired. It changes session attribution, not token counters or request IDs.

The database continues to use the existing synchronized SQLite connection. Cache hits avoid the event scan and its connection lock; uncached queries and writes still share that lock.

## Frontend validation (Windows, 2026-09-28)

The production client build contains 245 modules. Manifest entries confirm that History (223,548 bytes; gzip 74,634), Sessions (33,585; gzip 10,352), Settings (38,487; gzip 9,600), the browser mock (21,214; gzip 6,710), and the session mock (5,097; gzip 2,355) are dynamic entry chunks. These are module sizes, not measured startup savings; shared dependencies are excluded. Native windows no longer initialize browser mock data, and the dashboard loads heavy tabs on demand.

All 86 pure unit tests passed; svelte-check reported zero errors and warnings; the static production build completed. Six read-only Chromium render checks passed without simulated mouse/keyboard input: dark/light session details, Chinese narrow layouts, bounded history rows, Chart.js instance reuse after a synthetic ingestion event, AI/user-reviewed report evidence, and a 15,480-character preview without horizontal overflow. History tables render at most 100 data rows per page; session lists request 25 rows per page. Background snapshot/settings reconciliation pauses while hidden and resumes on visibility/focus; metadata-only completed scans refresh session summaries without replacing in-progress form drafts.

Run `node node_modules/@playwright/test/cli.js test --config=tests/render.config.ts` for the no-input checks. Set `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH` when using an existing browser installation. The test server is loopback-only and closes through a test-only shutdown endpoint; no debugging route is added to the shipped app.

## v0.5.0 backend measurements

Measured on Windows x64 (Windows 10 build 19045), Rust 1.98.1, `test` profile with debug information and no optimization, China Standard Time. SQLite used an in-memory database; transcript fixtures were ordinary local temporary files. The initial metadata index was built before timing append processing. The two benchmarks ran sequentially using `--test-threads=1`, with the OS file cache already warm. These results describe these synthetic workloads only.

```sh
cargo test --locked synthetic_ --lib -- --ignored --nocapture --test-threads=1
```

| Workload | Previous path | Updated path |
| --- | --- | --- |
| Usage-parser bytes for 20 small appends to a roughly 21 MB transcript | 423,636,195 bytes read from file starts | 2,970 appended bytes, plus at most 20,480 anchor bytes |
| Total elapsed time for those 20 appends | 3,233.195 ms, usage parsing only | 29.210 ms, including session metadata indexing and SQLite writes |
| Event rows scanned for 20 repeated history queries over 50,000 events | 1,000,000 | 0 after warming the aggregate cache |
| Total elapsed time for those 20 history queries | 6,476.621 ms | 1.126 ms |

Both benchmarks passed their counter/result correctness assertions. Usage-byte figures exclude the separate metadata index's reads on both paths; the updated timing includes that index. Setup, cold startup, application rendering, on-disk SQLite latency and production release performance are outside this benchmark. No startup or frame-rate improvement is inferred from these numbers.

## Polling floors and background work (v0.6)

- Quota polling: `refreshIntervalSec` (default 60, minimum 15). Claude is never polled more often than every 120 s, **per account**: each extra account is its own provider instance with its own clock, so N Claude accounts cost N requests per 120 s at most. With `adaptiveRefresh`, a provider whose logs have been quiet for 10 min doubles its interval, after 30 min it is x5, capped at 10 min. Every `429` raises a learned multiplier (cap x8, 15 min) that relaxes only after five good polls. Opening the dashboard or an explicit refresh ends the idle stretch.
- Retention: quota samples are kept `quotaRetentionDays` days (default 365, 0 = forever, maximum 3650) and thinned to one peak per hour after 14 days; `usage_events` are never deleted.
- Alerts run on the snapshot the scheduler already has (no extra requests) plus one 5-minute timer for the budget and weekly-summary checks. The tray rebuilds menu items, tooltip and icon only when their content or severity changes. `snapshot.json` is one atomic write per snapshot, and only while `exportSnapshot` is on.
- Window and hover timers are generation-counted tasks; there is no polling of window state except the 5 s geometry watchdog (off under layer-shell).

## Decision support and upgrade safety (v0.7)

- Routing advice is a pure function over the in-memory snapshot and its forecasts: no I/O, no extra provider request. The advice notification is claimed once per source reset period (a small in-memory map).
- The commit view (`gitAttribution`, off by default) starts no process while off. When on, opening the view for one project runs one read-only `git log` (at most 500 commits, killed after 10 s, output capped at 4 MiB); results are cached for 5 minutes (16 entries), with the window end rounded to 5 minutes so reopening the view hits the cache. Nothing runs in the background.
- The plan advisor runs in the webview over the quota cycles the History tab already built.
- A schema migration first writes a `VACUUM INTO` copy of `usage.db` plus a settings copy and keeps the newest 3 (`backups/`). This happens once per upgrade, on start-up before the database opens for use; its duration was not measured.
- Extra accounts add log roots to the existing watcher and one more request clock per account; `usage_events` gains one indexed column (`idx_usage_account_ts`).

## Robustness and maintenance (v0.6)

- The log watcher re-evaluates its roots every 60 s with one `read_dir`/metadata check per root; no polling of file contents is added. Watcher errors and root changes queue the existing bounded full reconciliation.
- Quota-sample maintenance runs once per day on the blocking pool: one retention `DELETE`, one hourly-peak thinning `DELETE` (window function over rows older than 14 days), a WAL checkpoint and `PRAGMA optimize`. It holds the shared connection lock for the duration of those statements; recent rows and `usage_events` are not touched. Verified by `retention_deletes_old_samples_and_zero_keeps_everything` and `old_samples_are_thinned_to_the_hourly_peak_per_series`; no timing was measured.
- Forecast attachment takes the connection lock once per refresh instead of once per window.
- A transient provider failure adds at most one extra request and ~1.5 s inside the same fetch.
