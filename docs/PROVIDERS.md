# Provider research — candidates beyond Claude Code and Codex

Researched 2026-09-21 for v0.2.0. **Nothing in this file was tested against a
live account**: the research machine has none of these tools installed or
signed in (`~/.gemini`, `~/.cursor`, `~/.config/github-copilot` do not exist).
Every factual claim below is a citation to published source code or official
documentation that was actually fetched; where a claim could not be backed by a
source it says **NOT VERIFIED**.

## The bar a candidate has to clear

Claude and Codex both work the same way: the app reads the credential the
tool's *own* CLI already wrote for itself, and calls the same endpoint that
CLI calls, with the same headers. That is the bar:

* **Acceptable** — an official, or at least first-party-client-documented,
  endpoint reached with the tool's own stored OAuth credential, used the way
  the tool itself uses it.
* **Not acceptable** — lifting a session credential out of another app's
  private database to replay its *web dashboard* session, or defeating an
  app's own encryption of its credential store.

| Candidate | Verdict | Shipped in this repo |
|---|---|---|
| GitHub Copilot | **implementable with evidence** | yes, `experimental` (see §1) |
| Google Gemini CLI | evidence for the HTTP call, **blocked on credential storage** | no (see §2) |
| Cursor | **not implemented — ToS and stability** | no (see §3) |

---

## 1. GitHub Copilot — implementable with evidence

### 1.1 Where the credential lives

The Copilot editor plugins (VS Code, JetBrains, Neovim) write a GitHub OAuth
token to a plain JSON file keyed by host:

```swift
static let editorAppsPath  = "~/.config/github-copilot/apps.json"
static let editorHostsPath = "~/.config/github-copilot/hosts.json"
```
— [openusage `CopilotAuthStore.swift` L30–L31](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotAuthStore.swift#L30-L31)

`apps.json` is the current file, `hosts.json` the older one; both are tried in
that order ([same file, L54](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotAuthStore.swift#L54)).

The JSON is an object keyed by host — `"github.com"` in the old file,
`"github.com:<appId>"` in the new one (the app id is Copilot's public client
id `Iv1.b507a08c87ecfe98`) — and each value carries `oauth_token`:

```swift
for (key, value) in object where key == "github.com" || key.hasPrefix("github.com:") {
    if let token = token(in: value) { return token }
}
```
— [openusage `CopilotAuthStore.swift` L111–L131](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotAuthStore.swift#L111-L131)

Only `github.com` entries may be used: a GitHub Enterprise entry's token must
not be sent to `api.github.com` (same source, doc comment above L111).

Per-OS directory resolution, from a Rust client:

```rust
if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
    PathBuf::from(xdg).join("github-copilot")
} else if cfg!(windows) {
    let local_app_data = std::env::var("LOCALAPPDATA")…;
    PathBuf::from(local_app_data).join("github-copilot")
} else {
    PathBuf::from(home).join(".config").join("github-copilot")
}
```
— [jcode `crates/jcode-base/src/auth/copilot.rs` L442–L462](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-base/src/auth/copilot.rs#L442-L462)
(macOS uses the `$HOME/.config` branch — there is no `~/Library/Application Support`
location for this file.) The same file also records the official Copilot CLI's
own plaintext credential at `~/.copilot/config.json`
([L147–L155](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-base/src/auth/copilot.rs#L147-L155)).

Other clients additionally fall back to the GitHub CLI's own token
(`~/.config/gh/hosts.yml`, or the `gh:github.com` keychain item) —
[openusage `CopilotAuthStore.swift` L32–L33](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotAuthStore.swift#L32-L33).
**This app deliberately does not do that**: `gh`'s token is a general-purpose
GitHub credential, not one Copilot tooling left for Copilot's own use.

### 1.2 Quota endpoint

```swift
static let usageURL = "https://api.github.com/copilot_internal/user"
…
headers: [
    "Authorization": "token \(token)",
    "Accept": "application/json",
    "Editor-Version": "vscode/1.96.2",
    "Editor-Plugin-Version": "copilot-chat/0.26.7",
    "User-Agent": "GitHubCopilotChat/0.26.7",
    "X-Github-Api-Version": "2025-04-01"
]
```
— [openusage `CopilotUsageClient.swift` L7–L29](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageClient.swift#L7-L29)

`GET`, and `Authorization` uses the **`token`** scheme, not `Bearer` (source
comment, [L3–L5](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageClient.swift#L3-L5)).

A second, independent client declares the full response type:

```ts
interface CopilotUsageResponse {
  access_type_sku: string
  analytics_tracking_id: string
  assigned_date: string
  can_signup_for_limited: boolean
  chat_enabled: boolean
  copilot_plan: string
  organization_login_list: Array<unknown>
  organization_list: Array<unknown>
  quota_reset_date: string
  quota_snapshots: QuotaSnapshots
}
interface QuotaSnapshots { chat: QuotaDetail; completions: QuotaDetail; premium_interactions: QuotaDetail }
interface QuotaDetail {
  entitlement: number
  overage_count: number
  overage_permitted: boolean
  percent_remaining: number
  quota_id: string
  quota_remaining: number
  remaining: number
  unlimited: boolean
}
```
— [copilot-api `src/services/github/get-copilot-usage.ts`](https://github.com/ericc-ch/copilot-api/blob/0ea08febdd7e3e055b03dd298bf57e669500b5c1/src/services/github/get-copilot-usage.ts)
(it fetches `${GITHUB_API_BASE_URL}/copilot_internal/user`, with
`GITHUB_API_BASE_URL = "https://api.github.com"` in
[`src/lib/api-config.ts`](https://github.com/ericc-ch/copilot-api/blob/0ea08febdd7e3e055b03dd298bf57e669500b5c1/src/lib/api-config.ts)).

**Strength of evidence: strong.** Two independent open-source clients, written
in different languages, agree on the URL, the auth scheme and the field names,
and one of them is itself a maintained usage monitor. It is corroborated by
GitHub's *own documentation* for the Copilot SDK, which exposes the same data
through `account.getQuota()` with a `quotaSnapshots` map keyed by
`premium_interactions` / `chat` / `completions` and fields
`entitlementRequests`, `usedRequests`, `remainingPercentage`, `resetDate` —
[GitHub Docs, Copilot SDK: usage and billing](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing).
The endpoint itself is still **undocumented**, exactly like the two endpoints
this app already relies on.

### 1.3 Response semantics (the parts that are easy to get wrong)

From openusage's mapper, which has been through several bug-fix rounds against
real accounts ([`CopilotUsageMapper.swift`](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageMapper.swift)):

* Buckets report percent **remaining**; a "used" meter is `100 - percent_remaining`
  ([L112](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageMapper.swift#L112)).
* `unlimited: true`, or the `-1` sentinel on `entitlement`/`remaining`, means
  "no meter" — paid plans send that for `chat` and `completions`
  ([L105](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageMapper.swift#L105)).
* `entitlement: 0` is a placeholder (org-managed seat, or `premium_interactions`
  on a free account) and must **not** be rendered as "0 % used"
  ([L109](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageMapper.swift#L109)).
* Reset time is `quota_reset_date`, with `limited_user_reset_date` on the older
  free-tier shape; both a bare date (`"2099-07-01"`) and an ISO-8601 datetime
  have been seen ([L39–L40](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageMapper.swift#L39-L40)).
* Older responses predate `quota_snapshots` and instead carry
  `limited_user_quotas` (remaining) against `monthly_quotas` (total)
  ([L62–L69](https://github.com/robinebers/openusage/blob/7caf4caab4970701ccaeae3798a71e9995847001/Sources/OpenUsage/Providers/Copilot/CopilotUsageMapper.swift#L62-L69)).
* The quota period is a **month**, not 5 hours or a week — so every window is
  `WindowKind::Other` in this app's model.

### 1.4 Token exchange (not needed here, recorded for completeness)

The OAuth token is exchanged for a short-lived Copilot bearer token at
`https://api.github.com/copilot_internal/v2/token` with
`authorization: token <oauth>`, returning `token`, `expires_at` and an
`endpoints` map — [litellm `github_copilot/authenticator.py`](https://github.com/BerriAI/litellm/blob/main/litellm/llms/github_copilot/authenticator.py),
[opencode-copilot-enhanced `src/token.ts`](https://github.com/kevingatera/opencode-copilot-enhanced/blob/main/src/token.ts)
(the latter also sends `Accept: application/json` and
`X-GitHub-Api-Version: 2025-04-01`). **The quota endpoint does not need this
exchange** — it takes the stored OAuth token directly.

### 1.5 Documented alternative: none for an individual

The public REST API for Copilot is seat/billing administration only —
`/orgs/{org}/copilot/billing`, `/orgs/{org}/copilot/billing/seats`,
`/orgs/{org}/members/{username}/copilot` — all requiring organization
ownership or `manage_billing:copilot` / `read:org`, and **none** of them
returns the authenticated individual's own quota:
[GitHub Docs, REST API endpoints for Copilot user management](https://docs.github.com/en/rest/copilot/copilot-user-management).

### 1.6 Local session logs with token counts

**NOT VERIFIED — no evidence found that any Copilot client writes local files
with per-request token counts.** No ingestion is implemented for Copilot, and
its history tab will stay empty.

### 1.7 Stability and ToS

* `copilot_internal/user` is undocumented and can change or disappear without
  notice. openusage's own history shows it drifting (zero-entitlement
  placeholders, the June 2026 move to AI credits, `token_based_billing`).
* The call is read-only, is made with the user's own Copilot credential, sends
  the same headers Copilot's own client sends, and is made at most once per
  refresh interval (default 60 s).
* No ToS clause prohibiting this was found; equally, **no clause permitting it
  was found** — it is undocumented, which is why the provider ships behind an
  "experimental" flag and is off unless credentials are detected.

**Verdict: implementable with evidence.**

---

## 2. Google Gemini CLI — HTTP call evidenced, credential storage blocks it

Repository: [google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli)
(Apache-2.0). All line references are against
[`cfbcaa8`](https://github.com/google-gemini/gemini-cli/tree/cfbcaa8df13ea4610bb379b377b56d62980c0032).

### 2.1 Quota endpoint — this part is fine

There is a real per-user quota call:

```ts
export interface BucketInfo {
  remainingAmount?: string;
  remainingFraction?: number;
  resetTime?: string;
  tokenType?: string;
  modelId?: string;
}
export interface RetrieveUserQuotaResponse { buckets?: BucketInfo[] }
```
— [`packages/core/src/code_assist/types.ts` L250–L265](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/types.ts#L250-L265)

It is a `POST` to `<endpoint>/<version>:<method>`:

```ts
export const CODE_ASSIST_ENDPOINT = 'https://cloudcode-pa.googleapis.com';
export const CODE_ASSIST_API_VERSION = 'v1internal';
…
getMethodUrl(method: string): string { return `${this.getBaseUrl()}:${method}`; }
```
— [`server.ts` L73–L74](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/server.ts#L73-L74),
[L524–L534](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/server.ts#L524-L534)

so the full request is
`POST https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota`,
`Content-Type: application/json`, body `{"project": "<projectId>"}`, authorised
by the Google OAuth access token
([`server.ts` L367–L374](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/server.ts#L367-L374),
[L415–L443](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/server.ts#L415-L443)).
The CLI skips the call entirely when it has no project id, and derives
`usedPercent` from `remainingFraction`
([`packages/core/src/config/config.ts`, `refreshUserQuota`](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/config/config.ts#L2305-L2340)).

`loadCodeAssist` (same base URL, method `loadCodeAssist`) is the read-only call
that yields the project id and the plan: `cloudaicompanionProject`,
`currentTier`/`paidTier` with ids `free-tier` / `legacy-tier` / `standard-tier`
— [`setup.ts` L174–L239](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/setup.ts#L174-L239),
[`types.ts` L83–L156](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/types.ts#L83-L156).
`GOOGLE_CLOUD_PROJECT` / `GOOGLE_CLOUD_PROJECT_ID` override it
([`setup.ts` L129–L132](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/setup.ts#L129-L132)).
So a quota refresh costs **two** undocumented POSTs, the second depending on
the first. (`onboardUser` is the call with side effects; it is not needed.)

### 2.2 Credential storage — this is the blocker

The plaintext file everyone remembers is legacy:

```ts
export const OAUTH_FILE = 'oauth_creds.json';
…
static getOAuthCredsPath(): string {
  return path.join(Storage.getGlobalGeminiDir(), OAUTH_FILE);   // ~/.gemini/oauth_creds.json
}
```
— [`packages/core/src/config/storage.ts` L22](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/config/storage.ts#L22),
[L255–L257](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/config/storage.ts#L255-L257)

but it is only read once, to migrate into the new store, and **deleted
afterwards**:
[`oauth-credential-storage.ts`, `migrateFromFileStorage`](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/code_assist/oauth-credential-storage.ts).
The live store is a `HybridTokenStorage` under service name
`gemini-cli-oauth`, account `main-account` (same file), which resolves to:

1. the **OS keychain** (`@github/keytar`-style native backend — Secret Service
   on Linux, Keychain on macOS, Credential Manager on Windows)
   — [`services/keychainService.ts`](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/services/keychainService.ts);
2. otherwise `~/.gemini/gemini-credentials.json`, **AES-256-GCM encrypted**
   with a key derived from the machine's hostname and username:

```ts
this.tokenFilePath = path.join(configDir, 'gemini-credentials.json');
…
const salt = `${os.hostname()}-${os.userInfo().username}-gemini-cli`;
return crypto.scryptSync('gemini-cli-oauth', salt, 32);
```
— [`services/fileKeychain.ts` L19–L26](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/services/fileKeychain.ts#L19-L26)

Reading (1) is arguably within this project's existing bar — the Claude
provider already reads a macOS Keychain item — but it needs a keyring
dependency and a working Secret Service on Linux, on three platforms, none of
which can be exercised on the development machine. Reading (2) means
re-implementing Gemini CLI's own obfuscation of a store it chose to encrypt,
which is over the line. Either way, **nothing here can be tested**: no Google
account, no `~/.gemini`, and both the credential read and the two-step API call
would have to be written blind.

### 2.3 Local session logs

`~/.gemini/tmp/<project-hash>/logs.json` exists but carries only user messages:

```ts
export interface LogEntry {
  sessionId: string; messageId: number; timestamp: string;
  type: MessageSenderType; message: string;
}
```
— [`packages/core/src/core/logger.ts` L15–L27](https://github.com/google-gemini/gemini-cli/blob/cfbcaa8df13ea4610bb379b377b56d62980c0032/packages/core/src/core/logger.ts#L15-L27)
— **no token counts**.

Token counts exist only in OpenTelemetry output, which is **disabled by
default** (`telemetry.enabled: false`) and only lands on disk when the user
sets `telemetry.target: "local"` with `telemetry.outfile`. The metric is
`gemini_cli.token.usage` with types `input` / `output` / `thought` / `cache` /
`tool` and a `model` attribute, and the `gemini_cli.api_response` event carries
`input_token_count`, `output_token_count`, `cached_content_token_count`,
`thoughts_token_count`, `tool_token_count` —
[gemini-cli `docs/cli/telemetry.md`](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/telemetry.md).
A log ingester for a file that does not exist on a default install is not worth
shipping.

### 2.4 Stability and ToS

`cloudcode-pa.googleapis.com/v1internal` is, by its own version string, an
internal API; it is not on Google's public API surface and has no published
contract. **NOT VERIFIED**: no Google ToS clause specifically addressing
third-party use of it was found. Published free-tier request limits for Gemini
CLI were **NOT VERIFIED** from an official page during this research.

**Verdict: the HTTP call is implementable with evidence; the provider is not,
because the credential cannot be obtained the way this app obtains credentials
without an untestable, cross-platform keyring dependency.** Revisit if the app
ever grows a keyring abstraction (the Claude macOS Keychain read is the seed of
one), or if a user with a Gemini account can capture one real response.

---

## 3. Cursor — not implemented (ToS and stability)

Researched and then **declined by the maintainer**. Recorded here so the
question is not re-opened without new information.

What the research found:

* A session JWT can be read out of Cursor's own SQLite state database
  (`~/.config/Cursor/User/globalStorage/state.vscdb`, table `ItemTable`, key
  `cursorAuth/accessToken`), the web session cookie rebuilt as
  `WorkosCursorSessionToken=<sub-tail>%3A%3A<jwt>`, and
  `GET https://cursor.com/api/usage-summary` called with it. Technically
  workable; entirely community reverse-engineering, not documented by Cursor.
* Cursor's [Terms of Service](https://cursor.com/terms-of-service) §1.5(viii)
  prohibits harvesting or scraping data from the service.
* The official Admin / Analytics APIs are **Enterprise-only**; there is no
  individual-user usage API ([docs.cursor.com](https://docs.cursor.com/)).
* No local Cursor file carries per-request token counts, so there is no
  local-logs fallback either.
* The endpoints sit behind Vercel bot protection that fingerprints TLS: a
  rustls client (which is what this app uses, via `reqwest`) gets a challenge
  page rather than JSON. They have also broken repeatedly (origin/CSRF 403s);
  the community `cursor-stats` project was abandoned over the churn.

Lifting another application's session credential out of its private database in
order to replay its web dashboard session is not the same thing as reading the
credential a CLI wrote for its own API client, and this project will not ship
it.

**Verdict: not implemented — ToS and stability.**
