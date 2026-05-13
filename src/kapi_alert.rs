use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::events::IncomingEvent;

pub const DEFAULT_STALE_REPEAT_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KapiAlertConfig {
    #[serde(default = "default_stale_repeat_secs")]
    pub stale_repeat_secs: u64,
}

impl Default for KapiAlertConfig {
    fn default() -> Self {
        Self {
            stale_repeat_secs: default_stale_repeat_secs(),
        }
    }
}

fn default_stale_repeat_secs() -> u64 {
    DEFAULT_STALE_REPEAT_SECS
}

#[derive(Debug, Default)]
pub struct KapiAlertDedupe {
    last_emitted: HashMap<String, u64>,
}

impl KapiAlertDedupe {
    pub fn should_emit_now(&mut self, event: &IncomingEvent, stale_repeat_secs: u64) -> bool {
        self.should_emit_at(event, unix_now_secs(), stale_repeat_secs)
    }

    pub fn should_emit_at(
        &mut self,
        event: &IncomingEvent,
        now_secs: u64,
        stale_repeat_secs: u64,
    ) -> bool {
        let Some(key) = dedupe_key(event) else {
            return true;
        };

        let repeat_allowed = worker_label(event.canonical_kind()) == Some("stale");
        match self.last_emitted.get(&key).copied() {
            Some(last_seen)
                if !repeat_allowed
                    || now_secs.saturating_sub(last_seen) < stale_repeat_secs.max(1) =>
            {
                false
            }
            _ => {
                self.last_emitted.insert(key, now_secs);
                true
            }
        }
    }
}

pub fn is_kapi_worker_event(kind: &str) -> bool {
    kind.starts_with("kapi.worker.")
}

pub fn worker_label(kind: &str) -> Option<&str> {
    kind.strip_prefix("kapi.worker.")
        .map(|label| label.trim())
        .filter(|label| !label.is_empty())
}

pub fn action_required(kind: &str, payload: &Value) -> bool {
    let label = worker_label(kind).unwrap_or_default();
    if matches!(
        label,
        "blocked" | "failed" | "stale" | "review-ready" | "candidate-ready" | "merge-ready"
    ) {
        return true;
    }

    let normalized_fields = ["reason", "status", "verdict", "review_state", "conclusion"]
        .into_iter()
        .filter_map(|key| string_field(payload, key))
        .map(|value| normalize_token(&value))
        .collect::<Vec<_>>();

    normalized_fields.iter().any(|value| {
        matches!(
            value.as_str(),
            "blocked"
                | "failed"
                | "stale"
                | "review-ready"
                | "candidate-ready"
                | "changes-requested"
                | "changes_requested"
                | "approved"
                | "merge-ready"
        )
    })
}

pub fn should_apply_mention(event: &IncomingEvent) -> bool {
    !is_kapi_worker_event(event.canonical_kind())
        || action_required(event.canonical_kind(), &event.payload)
}

pub fn render_worker_event(
    kind: &str,
    payload: &Value,
    alert_prefix: bool,
    inline: bool,
) -> String {
    let label = worker_label(kind).unwrap_or("event");
    let repo = string_field(payload, "repo").unwrap_or_else(|| "unknown-repo".to_string());
    let slug = string_field(payload, "slug").unwrap_or_else(|| "unknown-slug".to_string());
    let mode = string_field(payload, "mode").unwrap_or_else(|| "unknown-mode".to_string());
    let summary = string_field(payload, "summary");

    if inline {
        let mut rendered = format!("[kapi:{label}] {repo} {slug} ({mode})");
        if let Some(summary) = summary {
            rendered.push_str(" — ");
            rendered.push_str(&summary);
        }
        return rendered;
    }

    let mut lines = vec![if alert_prefix {
        format!("🚨 [kapi] {label}")
    } else {
        format!("[kapi] {label}")
    }];
    let recommended = recommended_action(payload, &slug);
    push_line(&mut lines, "repo", Some(repo));
    push_line(&mut lines, "slug", Some(slug));
    push_line(&mut lines, "branch", string_field(payload, "branch"));
    push_line(&mut lines, "mode", Some(mode));
    push_line(&mut lines, "reason", string_field(payload, "reason"));
    push_line(&mut lines, "summary", summary);
    push_line(&mut lines, "recommended", recommended);
    lines.join("\n")
}

fn dedupe_key(event: &IncomingEvent) -> Option<String> {
    let kind = event.canonical_kind();
    if !is_kapi_worker_event(kind) {
        return None;
    }

    let payload = &event.payload;
    let mut parts = vec![format!("kind={kind}")];
    for key in [
        "repo",
        "slug",
        "mode",
        "branch",
        "reason",
        "status",
        "summary",
        "recommended",
        "review_state",
        "verdict",
        "conclusion",
    ] {
        if let Some(value) = string_field(payload, key) {
            parts.push(format!("{key}={value}"));
        }
    }

    if action_required(kind, payload)
        && let Some(cursor) = string_field(payload, "cursor")
    {
        parts.push(format!("cursor={cursor}"));
    }

    Some(parts.join("\u{1f}"))
}

fn recommended_action(payload: &Value, slug: &str) -> Option<String> {
    string_field(payload, "recommended")
        .or_else(|| string_field(payload, "recommended_action"))
        .or_else(|| string_field(payload, "action"))
        .or_else(|| {
            string_field(payload, "repo_path")
                .map(|repo_path| format!("kapi report {slug} --from {repo_path} --json"))
        })
}

fn push_line(lines: &mut Vec<String>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        lines.push(format!("{key}: {value}"));
    }
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_token(value: &str) -> String {
    value.trim().replace(' ', "-").to_ascii_lowercase()
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn worker_event(kind: &str, payload: Value) -> IncomingEvent {
        IncomingEvent {
            kind: kind.into(),
            channel: None,
            mention: None,
            format: None,
            template: None,
            payload,
        }
    }

    #[test]
    fn suppresses_repeated_identical_worker_events() {
        let event = worker_event(
            "kapi.worker.blocked",
            json!({
                "repo": "devkade/kapi",
                "slug": "issue-78",
                "mode": "ralph",
                "reason": "blocked",
                "summary": "waiting for review",
                "cursor": "abc"
            }),
        );
        let mut dedupe = KapiAlertDedupe::default();

        assert!(dedupe.should_emit_at(&event, 100, DEFAULT_STALE_REPEAT_SECS));
        assert!(!dedupe.should_emit_at(&event, 101, DEFAULT_STALE_REPEAT_SECS));
    }

    #[test]
    fn allows_stale_repeat_after_configured_interval() {
        let event = worker_event(
            "kapi.worker.stale",
            json!({
                "repo": "devkade/kapi",
                "slug": "issue-78",
                "mode": "ralph",
                "reason": "stale",
                "summary": "no output",
                "cursor": "abc"
            }),
        );
        let mut dedupe = KapiAlertDedupe::default();

        assert!(dedupe.should_emit_at(&event, 100, 30));
        assert!(!dedupe.should_emit_at(&event, 120, 30));
        assert!(dedupe.should_emit_at(&event, 130, 30));
    }

    #[test]
    fn running_output_growth_does_not_create_new_dedupe_key() {
        let first = worker_event(
            "kapi.worker.running",
            json!({
                "repo": "devkade/kapi",
                "slug": "issue-78",
                "mode": "ralph",
                "summary": "still running",
                "cursor": "cursor-1",
                "output_tail": "one"
            }),
        );
        let second = worker_event(
            "kapi.worker.running",
            json!({
                "repo": "devkade/kapi",
                "slug": "issue-78",
                "mode": "ralph",
                "summary": "still running",
                "cursor": "cursor-2",
                "output_tail": "one two three"
            }),
        );
        let mut dedupe = KapiAlertDedupe::default();

        assert!(dedupe.should_emit_at(&first, 100, DEFAULT_STALE_REPEAT_SECS));
        assert!(!dedupe.should_emit_at(&second, 101, DEFAULT_STALE_REPEAT_SECS));
    }

    #[test]
    fn only_action_required_worker_events_keep_mentions() {
        let running = worker_event(
            "kapi.worker.running",
            json!({"repo": "devkade/kapi", "slug": "issue-78"}),
        );
        let blocked = worker_event(
            "kapi.worker.blocked",
            json!({"repo": "devkade/kapi", "slug": "issue-78"}),
        );

        assert!(!should_apply_mention(&running));
        assert!(should_apply_mention(&blocked));
    }
}
