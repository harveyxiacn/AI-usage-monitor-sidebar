# Session analysis / 会话分析

The Sessions dashboard connects token use to the task that produced it. All
indexing and normal statistics are local. AI evaluation is a separate, explicit
action and is never triggered by polling, ingestion or navigation.

## Find a session

Filter by provider, project or date, search saved names/project/identity, and sort by recent
activity, tokens or saved name. Prompt-derived title excerpts are displayed on demand;
their bodies are not indexed for search or title sorting. Pagination applies on the server across the matching
sessions, including small recent conversations. Session identity is provider plus
session ID; missing IDs are labelled unassigned.

Titles distinguish local aliases, native titles, prompt excerpts and a fallback.
Local aliases override native titles and do not rename anything in the provider's
CLI. Clearing an alias restores the next available title. Codex thread metadata
and session_index.jsonl names and Claude custom-title/summary records are read
when available; unknown/older formats fall back gracefully.

## Insights

The **Insights** segment of the Sessions tab summarises the sessions matching the
tab's date, project, provider and search filters: median cost, active time and
turns, tool failure and repeated-call rates, log-scale distributions with median
and P90, a turns-versus-cost scatter (point size is tool failures), ranked lists
and tool usage. It is computed from usage, timing and tool-name metadata only, so
it works with local content off. At most the newest 1000 matching sessions are
analysed. A session is flagged when it has at least 3 failed calls and 20% or
more of its calls fail, or at least 5 repeated calls and 30% or more repeated;
these are prompts to look, not proof of waste. History's Sessions table is a
summary that opens this tab with the same filters.

## Local content and evidence

Local content is **off by default**, including on upgrades. The background index
still parses local JSONL to identify roles, timing, titles, usage and tool events;
it stores byte references rather than copies of conversation bodies. Enabling
local content allows the UI to read bounded pages from those original files.
Deleted or rewritten sources produce a warning instead of stale or unrelated
text. Source logs remain owned by the CLI and may expire according to its policy.
The adapter indexes supported text records, skips malformed or oversized JSONL
lines (over 2 MiB), and does not interpret image/audio attachments. Message
display is capped at 12,000 characters per message and 120,000 per page; turn
details show at most the first 500 observed turns. These limits mean a transcript
view or assessment is not a guarantee of complete original conversation coverage.

Only genuine user messages count as user turns. Tool results and injected context
are not user requests. Per-turn usage is shown only when the provider's records
can be linked; unknown is not zero. Parent/child agents are linked when metadata
exists and each session shows its own usage, so opening a parent does not add
children's totals twice.

| Metric | Interpretation |
|---|---|
| Tokens and model calls | Recorded usage events, not the length/count of typed prompts |
| Cache share | Cached input divided by all recorded input, not a productivity score |
| Activity span | First-to-last observed activity, including idle time |
| Estimated active time | Consecutive observed gaps capped at five minutes; not measured labor time |
| Turn duration | Observed turn boundaries when available; no claim about generation speed |
| Tool failures/repeated calls | Recorded failures and repeated invocation signatures, not proof of wasted work |
| Estimated cost | API-equivalent comparison using configured prices, never a subscription invoice |

## Configure on-demand AI evaluation

1. Enable local content, then open Analysis settings in Sessions.
2. Enter a Chat Completions-compatible HTTPS endpoint and the model available to
   your account. The default endpoint is `https://api.openai.com/v1/chat/completions`;
   a model is deliberately not selected automatically. Loopback HTTP is supported
   for a local compatible service. The endpoint must support JSON-object output
   and `max_completion_tokens`.
3. Enter the **name** of the environment variable holding your API key (default
   `OPENAI_API_KEY`). Configure its value through your normal OS/launcher secret
   mechanism and restart the application so it inherits that environment. Never
   paste CLI OAuth credentials into this setting. A local service can omit a key.
4. Optionally enter input/output USD-per-million rates. Without both rates,
   evaluation cost remains unknown. Character-based token estimates are approximate.
5. Choose a session or selected turns, prepare the preview, review and edit it,
   then press Send. Keep message identifier headers for evidence citations.

The preview includes a bounded section of measured local token/tool/timing
statistics, with whole-session totals separated from selected turn evidence.
Additional turn metrics can be omitted to preserve space for the messages.
Reviewing or preparing this preview does not send a network request.

Common credential patterns are redacted before preview. This cannot recognize
every secret or sensitive business detail: the exact editable preview is the
content that will be sent. Automatic retries and redirects are disabled. Cancelling
waiting does not undo a request the service already received or its charges.

The response separates prompt strengths/gaps/suggestions, requirement assessments,
efficiency observations and limitations. Statuses are **verified, partial, unmet,
unknown**; AI judgments remain proposals until a user confirms or corrects them.
A model saying “done” does not prove acceptance criteria were met. Evidence links
refer to included message IDs; missing references cannot establish a verified
result. No synthetic completion percentage is produced.
Changing a requirement's status or explanation records it as a human revision;
the original AI assessment is retained locally alongside the reviewed report.

Identical reviewed content, source revision, settings and rubric can reuse the
saved result without another paid call. Evaluator token usage and configured-rate
cost are recorded separately from coding usage. The most recent 100 reports and
their preview cache keys are retained locally. Clear analysis removes the selected
session's assessments and alias; turning local content off removes all reports.
Neither action deletes provider logs or token history.

## 中文使用说明

“洞察”视图汇总符合当前筛选的会话：费用、活跃时间、轮次的中位数，工具失败率与重复调用率，对数分布（含中位数和 P90），轮次与费用散点图（点大小为工具失败数），排行榜与工具使用情况。仅使用用量、时间和工具名称元数据，不读取对话内容；最多分析最近 1000 个会话。标记条件为失败至少 3 次且占 20% 以上，或重复至少 5 次且占 30% 以上，仅作提示。历史页的“会话”表现为摘要，并带着相同筛选跳转到会话页。

“会话”页面将 Token 消耗关联到具体任务，支持真正的后端分页、日期/项目/服务商筛选、已保存名称与标识搜索，
以及最近活动、消耗、已保存名称排序。名称会区分本地别名、原生标题、首条需求摘录与默认名称；本地别名不修改 CLI。
需求摘录仅在开启内容后按需显示，不参与名称搜索和排序。

本地内容默认关闭。后台仍解析日志建立计数和消息位置索引，但不复制对话正文；开启后按页读取原始文件。
日志被删除或改写时会提示。工具返回和自动注入上下文不算用户提问，无法关联的轮次 Token 显示未知。
父子 Agent 分别显示自身用量，避免重复累计。活动跨度包含空闲，活跃时间只是将相邻间隔截断到五分钟的估算。

AI 评测需要自行配置兼容 Chat Completions 的接口和模型；API Key 通过环境变量提供，界面只填写变量名。
先选择会话或轮次，生成预览，检查脱敏和发送内容，再主动发送。预览中的消息标识用于证据定位，应保留。
模型按需求逐项给出已验证、部分满足、未满足或未知，并附证据；你可以修正状态并确认。它不会自动生成
看似客观的完成百分比，也不会把高 Token 消耗直接判为低效率。

报告缓存避免重复付费，记录评测自身的 Token 和估算费用。最多保留最近 100 份报告及预览缓存键。
清除分析不删除源日志或 Token 历史；关闭本地内容会清除全部评测报告。
