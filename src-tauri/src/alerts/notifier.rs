//! Notification fan-out: the native OS notification and an optional webhook.
//!
//! Every alert goes through [`deliver`], the single gate:
//!
//! * `notifications` is the master switch;
//! * focus mode (`crate::focus`) silences **all** channels, not just the
//!   native one — a do-not-disturb that still pings a phone through ntfy
//!   would be a surprise. The summary of a silenced alert is not queued; the
//!   threshold/forecast/budget bookkeeping simply tries again on its next
//!   evaluation, and the weekly summary retries until Monday ends;
//! * [`send_test`] is the explicit exception: a user pressing "Send test"
//!   wants to see the channel work, so it ignores both gates.
//!
//! The webhook URL may carry a secret (a Slack token, a private ntfy topic).
//! It is validated as `https://` only, sent with a 5 s timeout and **no
//! redirects** (a redirect would hand the payload to a host the user never
//! typed), and never written to the log or to an error shown in the UI — only
//! its host is.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::model::{Settings, WebhookKind};

/// Where an alert can go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Native,
    Webhook,
}

impl Channel {
    pub fn parse(s: &str) -> Option<Channel> {
        match s {
            "native" => Some(Channel::Native),
            "webhook" => Some(Channel::Webhook),
            _ => None,
        }
    }
}

/// What the alert is about; the webhook reports it as `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Warn,
    Critical,
    Forecast,
    Budget,
    Summary,
    Test,
}

/// One notification, already in the user's language.
#[derive(Clone, Debug)]
pub struct Alert {
    pub title: String,
    pub body: String,
    /// Provider id (`claude`, `codex`), when the alert belongs to one.
    pub provider: Option<String>,
    /// Human name of the quota window, when it belongs to one.
    pub window: Option<String>,
    pub level: Level,
}

impl Alert {
    pub fn new(level: Level, title: impl Into<String>, body: impl Into<String>) -> Self {
        Alert {
            title: title.into(),
            body: body.into(),
            provider: None,
            window: None,
            level,
        }
    }

    pub fn for_window(mut self, provider: &str, window: &str) -> Self {
        self.provider = Some(provider.to_string());
        self.window = Some(window.to_string());
        self
    }
}

/// Body of a webhook request, built without touching the network.
#[derive(Debug, PartialEq, Eq)]
pub struct WebhookRequest {
    pub content_type: &'static str,
    pub body: String,
    /// `Title` header (ntfy only), already safe for an HTTP header.
    pub title_header: Option<String>,
}

/// JSON of the `generic` kind.
#[derive(Serialize)]
struct GenericPayload<'a> {
    title: &'a str,
    body: &'a str,
    provider: Option<&'a str>,
    window: Option<&'a str>,
    level: Level,
    /// RFC 3339 UTC.
    ts: String,
}

/// RFC 2047 encoded-word when `title` is not plain ASCII (ntfy decodes it).
fn header_safe(title: &str) -> String {
    let printable = title.chars().all(|c| (' '..='~').contains(&c));
    if printable {
        title.to_string()
    } else {
        format!(
            "=?UTF-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode(title)
        )
    }
}

/// Wire format for `kind`. Pure, so the three formats are unit-tested.
pub fn build_request(kind: WebhookKind, alert: &Alert, ts: &str) -> WebhookRequest {
    match kind {
        WebhookKind::Generic => WebhookRequest {
            content_type: "application/json",
            body: serde_json::to_string(&GenericPayload {
                title: &alert.title,
                body: &alert.body,
                provider: alert.provider.as_deref(),
                window: alert.window.as_deref(),
                level: alert.level,
                ts: ts.to_string(),
            })
            .unwrap_or_default(),
            title_header: None,
        },
        WebhookKind::Ntfy => WebhookRequest {
            content_type: "text/plain; charset=utf-8",
            body: alert.body.clone(),
            title_header: Some(header_safe(&alert.title)),
        },
        WebhookKind::Slack => WebhookRequest {
            content_type: "application/json",
            body: serde_json::json!({ "text": format!("*{}*\n{}", alert.title, alert.body) })
                .to_string(),
            title_header: None,
        },
    }
}

/// `host` of a webhook URL — the only part that is ever logged or shown.
pub fn redacted_host(url: &str) -> String {
    reqwest::Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .map(|h| format!("{h}/…"))
        .unwrap_or_else(|| "(invalid URL)".to_string())
}

/// A client that never follows a redirect and gives up after 5 s.
fn webhook_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

/// POST `alert` to the configured webhook. Errors never contain the URL.
pub async fn send_webhook(settings: &Settings, alert: &Alert) -> Result<()> {
    let url = crate::commands::pricing::validate_url(&settings.webhook.url)
        .map_err(|_| anyhow!("the webhook URL must be an https:// address"))?;
    let host = redacted_host(url.as_str());
    let ts = chrono::Utc::now().to_rfc3339();
    let req = build_request(settings.webhook.kind, alert, &ts);
    let mut builder = webhook_client()
        .post(url)
        .header("Content-Type", req.content_type)
        .body(req.body);
    if let Some(title) = &req.title_header {
        builder = builder.header("Title", title);
    }
    let response = builder.send().await.map_err(|e| {
        // `without_url` drops the address reqwest would otherwise print.
        let e = e.without_url();
        let reason = if e.is_timeout() {
            "timed out"
        } else if e.is_connect() {
            "could not connect"
        } else {
            "request failed"
        };
        anyhow!("webhook {host}: {reason}")
    })?;
    let status = response.status();
    if status.is_redirection() {
        bail!(
            "webhook {host}: refused to follow a redirect (HTTP {})",
            status.as_u16()
        );
    }
    if !status.is_success() {
        bail!("webhook {host}: HTTP {}", status.as_u16());
    }
    Ok(())
}

/// Show `alert` as an OS notification.
pub fn send_native(app: &AppHandle, alert: &Alert) -> Result<()> {
    app.notification()
        .builder()
        .title(alert.title.clone())
        .body(alert.body.clone())
        .show()
        .map_err(|e| anyhow!("{e}"))
}

/// Which channels would receive an alert under `settings`, or none when the
/// master switch is off or focus mode is on. Pure.
pub fn channels_for(settings: &Settings, now_ms: i64) -> Vec<Channel> {
    if !settings.notifications || !crate::focus::notifications_allowed(settings, now_ms) {
        return Vec::new();
    }
    let mut out = vec![Channel::Native];
    if settings.webhook.enabled
        && crate::commands::pricing::validate_url(&settings.webhook.url).is_ok()
    {
        out.push(Channel::Webhook);
    }
    out
}

/// Fan an alert out to the enabled channels. Returns `false` when nothing was
/// attempted (master switch off or focus mode), so a caller that keeps
/// once-per-period bookkeeping can retry later instead of losing the alert.
pub fn deliver(app: &AppHandle, settings: &Settings, alert: Alert) -> bool {
    let channels = channels_for(settings, super::now_ms());
    if channels.is_empty() {
        return false;
    }
    log::info!(
        "notification ({:?}): {} — {}",
        alert.level,
        alert.title,
        alert.body
    );
    for channel in channels {
        match channel {
            Channel::Native => {
                if let Err(e) = send_native(app, &alert) {
                    log::warn!("could not show the notification: {e:#}");
                }
            }
            Channel::Webhook => {
                let settings = settings.clone();
                let alert = alert.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = send_webhook(&settings, &alert).await {
                        log::warn!("{e:#}");
                    }
                });
            }
        }
    }
    true
}

/// "Send test" button: one channel, no gates, errors go back to the UI.
pub async fn send_test(app: &AppHandle, channel: Channel, chinese: bool) -> Result<()> {
    let settings = crate::window::settings_of(app);
    let alert = if chinese {
        Alert::new(
            Level::Test,
            "AI Usage Sidebar 测试通知",
            "如果你看到这条消息，通知渠道工作正常。",
        )
    } else {
        Alert::new(
            Level::Test,
            "AI Usage Sidebar test notification",
            "If you can read this, the notification channel works.",
        )
    };
    match channel {
        Channel::Native => send_native(app, &alert),
        Channel::Webhook => send_webhook(&settings, &alert).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert() -> Alert {
        Alert::new(Level::Warn, "Claude 5-hour limit", "At 72 %").for_window("claude", "5-hour")
    }

    #[test]
    fn generic_is_json_with_every_documented_field() {
        let req = build_request(WebhookKind::Generic, &alert(), "2026-09-15T00:00:00+00:00");
        assert_eq!(req.content_type, "application/json");
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "title": "Claude 5-hour limit",
                "body": "At 72 %",
                "provider": "claude",
                "window": "5-hour",
                "level": "warn",
                "ts": "2026-09-15T00:00:00+00:00",
            })
        );
        // alerts without a window carry explicit nulls, not missing keys
        let v: serde_json::Value = serde_json::from_str(
            &build_request(
                WebhookKind::Generic,
                &Alert::new(Level::Budget, "t", "b"),
                "x",
            )
            .body,
        )
        .unwrap();
        assert!(v["provider"].is_null() && v["window"].is_null());
        assert_eq!(v["level"], "budget");
    }

    #[test]
    fn ntfy_is_plain_text_with_a_title_header() {
        let req = build_request(WebhookKind::Ntfy, &alert(), "x");
        assert_eq!(req.body, "At 72 %");
        assert_eq!(req.title_header.as_deref(), Some("Claude 5-hour limit"));
        assert!(req.content_type.starts_with("text/plain"));
    }

    #[test]
    fn a_non_ascii_ntfy_title_is_rfc2047_encoded() {
        let zh = Alert::new(Level::Warn, "Claude 5 小时限额", "已用 72%");
        let header = build_request(WebhookKind::Ntfy, &zh, "x")
            .title_header
            .unwrap();
        assert!(header.starts_with("=?UTF-8?B?") && header.ends_with("?="));
        assert!(header.is_ascii());
        assert!(reqwest::header::HeaderValue::from_str(&header).is_ok());
    }

    #[test]
    fn slack_wraps_everything_in_text() {
        let req = build_request(WebhookKind::Slack, &alert(), "x");
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(v["text"], "*Claude 5-hour limit*\nAt 72 %");
        assert_eq!(v.as_object().unwrap().len(), 1);
    }

    #[test]
    fn only_the_host_of_a_webhook_url_is_ever_shown() {
        let shown = redacted_host("https://hooks.slack.com/services/T000/B000/SECRETSECRET");
        assert_eq!(shown, "hooks.slack.com/…");
        assert!(!shown.contains("SECRET"));
        assert_eq!(redacted_host("not a url"), "(invalid URL)");
    }

    #[tokio::test]
    async fn a_failing_request_does_not_leak_the_url() {
        let mut s = Settings::default();
        // port 9 (discard) on loopback over https: refused or failing fast
        s.webhook.url = "https://127.0.0.1:9/hook/TOPSECRET".into();
        let err = send_webhook(&s, &alert()).await.unwrap_err().to_string();
        assert!(!err.contains("TOPSECRET"), "{err}");
        assert!(!err.contains("/hook"), "{err}");
        s.webhook.url = "http://example.com/x".into();
        let err = send_webhook(&s, &alert()).await.unwrap_err().to_string();
        assert!(err.contains("https"), "{err}");
    }

    #[test]
    fn focus_mode_and_the_master_switch_silence_every_channel() {
        let mut s = Settings::default();
        s.webhook.enabled = true;
        s.webhook.url = "https://ntfy.sh/topic".into();
        assert!(
            channels_for(&s, 5).is_empty(),
            "notifications are off by default"
        );
        s.notifications = true;
        assert_eq!(channels_for(&s, 5), vec![Channel::Native, Channel::Webhook]);
        s.focus_until = 100;
        assert!(
            channels_for(&s, 5).is_empty(),
            "focus silences the webhook too"
        );
        assert_eq!(channels_for(&s, 100).len(), 2, "…until it ends");
        s.focus_until = 0;
        s.webhook.url = "http://insecure.example/x".into();
        assert_eq!(
            channels_for(&s, 5),
            vec![Channel::Native],
            "http is never used"
        );
        s.webhook.enabled = false;
        assert_eq!(channels_for(&s, 5), vec![Channel::Native]);
    }

    #[test]
    fn channel_names_parse() {
        assert_eq!(Channel::parse("native"), Some(Channel::Native));
        assert_eq!(Channel::parse("webhook"), Some(Channel::Webhook));
        assert_eq!(Channel::parse("email"), None);
    }
}
