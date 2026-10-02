# Status lines and scripts

AI Usage Sidebar can hand its numbers to anything that can run a command: Claude
Code's status line, tmux, polybar, waybar, a shell prompt.

It works in two steps, and neither needs the network or your credentials:

1. **The app writes a snapshot file.** Turn on *Settings -> Integrations -> Export
   snapshot file* (or `"exportSnapshot": true` in `settings.json`). After every
   refresh the app replaces `snapshot.json` in its data directory, atomically.
2. **`ai-usage-sidebar --print` reads that file** and exits. It does not start the
   app, does not open a window and does not disturb a running instance.

The app has to be running for the numbers to stay current. A snapshot older than
15 minutes counts as stale (see *Pause polling* below).

## The command

```
ai-usage-sidebar --print [--format json|line|statusline] [--provider claude]
```

| Format | Example |
|---|---|
| `line` (default) | `Claude 5h 73% (1h12m) · 7d 41% \| Codex 5h 12%` |
| `statusline` | `CC 5h 73% 7d 41% \| CX 5h 12%` |
| `json` | the snapshot (see below) on one line; filtered by `--provider` when given |

* Percentages are **used** percent, per account-wide window. `(1h12m)` is the time
  until the first window resets. In `statusline`, a window at 90 % or more gets a
  trailing `!`, and a provider that needs attention (signed out, token expired,
  error) a trailing `?`. Per-model windows appear in `json` only.
* `--provider claude` (or `codex`, `copilot`, `openrouter`) limits the output to one provider.
  Extra accounts (Settings -> Accounts) have their own ids: `--provider claude@work`.
  `--provider claude` means the primary account only.
* In `statusline` a provider is abbreviated `CC` (Claude), `CX` (Codex), `GH` (Copilot),
  `OP` (OpenRouter); an extra account adds its id, `CC@work`.

Exit codes:

| Code | Meaning |
|---|---|
| `0` | printed |
| `1` | bad arguments |
| `2` | no usable data: the file is missing, unreadable, from an unknown schema, older than 15 minutes, or lacks the provider. The reason is on stderr, stdout stays empty. |

### Where the file lives

| OS | `snapshot.json` |
|---|---|
| Linux | `~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/snapshot.json` |
| macOS | `~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar/snapshot.json` |
| Windows | `%LOCALAPPDATA%\io.github.harveyxiacn.ai-usage-sidebar\snapshot.json` (or `%APPDATA%\...` if your database already lives there) |

The settings page shows the exact path.

### The JSON schema (version 1)

```json
{
  "schema": 1,
  "updatedAt": "2026-10-02T10:00:00Z",
  "providers": [
    {
      "id": "claude",
      "name": "Claude",
      "status": "ok",
      "plan": "Claude Max 5x",
      "fetchedAt": "2026-10-02T09:59:40Z",
      "windows": [
        {
          "kind": "five_hour",
          "label": "5 hour",
          "usedPercent": 73,
          "resetsAt": "2026-10-02T11:12:00Z",
          "forecast": { "projectedPercentAtReset": 96, "exhaustsAt": null }
        }
      ]
    }
  ]
}
```

* `id` is the provider id, or `provider@account` for an extra account, which also
  carries `"account": "work"` (absent for the primary account).
* `status` is `ok`, `not_logged_in`, `token_expired`, `rate_limited`, `error` or
  `disabled`. A non-`ok` provider keeps its last known windows when it has any.
* `kind` is `five_hour`, `seven_day` or `other`. Per-model windows carry a
  `scope`. `forecast` is present only when there is enough history.
* Account e-mails and names are never written. New fields may be added without
  changing `schema`; a breaking change bumps it.

## Claude Code `statusLine`

Add this to `~/.claude/settings.json` (create the file if needed). Claude Code
runs the command, pipes a JSON blob to its stdin (ignored here) and shows what it
prints.

Linux, with `ai-usage-sidebar` on your `PATH` (the `.deb`, `.rpm` and
`install-linux.sh` all put it there):

```json
{
  "statusLine": {
    "type": "command",
    "command": "ai-usage-sidebar --print --format statusline || echo 'usage n/a'"
  }
}
```

macOS:

```json
{
  "statusLine": {
    "type": "command",
    "command": "'/Applications/AI Usage Sidebar.app/Contents/MacOS/ai-usage-sidebar' --print --format statusline || echo 'usage n/a'"
  }
}
```

Windows (use the full path to the installed `.exe`; the one below is the usual
per-user location, adjust it to yours):

```json
{
  "statusLine": {
    "type": "command",
    "command": "\"C:\\Users\\YOU\\AppData\\Local\\AI Usage Sidebar\\ai-usage-sidebar.exe\" --print --format statusline"
  }
}
```

## tmux

```tmux
set -g status-interval 60
set -g status-right '#(ai-usage-sidebar --print --format statusline 2>/dev/null) %H:%M'
```

## polybar

```ini
[module/ai-usage]
type = custom/script
exec = ai-usage-sidebar --print --format statusline 2>/dev/null || echo "usage n/a"
interval = 60
```

## waybar

```jsonc
"custom/ai-usage": {
  "exec": "ai-usage-sidebar --print --format statusline 2>/dev/null || echo 'usage n/a'",
  "interval": 60
}
```

Add `"custom/ai-usage"` to one of the module lists (`modules-right`, ...).

## Notes

* **Windows terminals.** The release build is a GUI program, so a shell does not
  wait for it. Output still appears in the terminal you ran it from, but
  PowerShell and `cmd` return to the prompt first. To use the output in a script,
  wait for it:
  `Start-Process -Wait -NoNewWindow -FilePath ai-usage-sidebar.exe -ArgumentList '--print'`,
  or pipe it: `& ai-usage-sidebar.exe --print | Out-String`. Programs that spawn
  it and read its output, such as Claude Code, are not affected.
* **Pause polling.** With *Pause polling* on (Settings -> Integrations, or the tray
  menu) the app stops refreshing, so the snapshot ages and `--print` reports it as
  stale after 15 minutes. Use `--format json` and read `updatedAt` yourself if you
  want old numbers anyway.
* **Several providers.** Disabled providers are left out of `line` and `statusline`.
  Every enabled account of a provider gets its own entry.
