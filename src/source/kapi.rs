use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::Result;
use crate::client::DaemonClient;
use crate::events::{IncomingEvent, MessageFormat};

#[derive(Debug, Clone)]
pub struct KapiWatchOptions {
    pub from: PathBuf,
    pub channel: Option<String>,
    pub mention: Option<String>,
    pub stale_minutes: u64,
    pub format: MessageFormat,
    pub cursor_file: Option<PathBuf>,
    pub poll_interval_secs: u64,
    pub once: bool,
    pub discord_topic_id: Option<String>,
    pub require_discord_topic: bool,
}

impl KapiWatchOptions {
    pub fn resolved_cursor_file(&self) -> PathBuf {
        self.cursor_file.clone().unwrap_or_else(|| {
            let repo_key = sanitize_cursor_key(&self.from.to_string_lossy());
            default_cursor_dir().join(format!("{repo_key}.cursor"))
        })
    }
}

pub async fn watch_daemon(client: DaemonClient, options: KapiWatchOptions) -> Result<()> {
    validate_options(&options)?;
    loop {
        let events = poll_kapi_events(&options).await?;
        for event in &events.events {
            client.send_event(event).await?;
        }
        if let Some(cursor) = events.cursor {
            write_cursor(&options.resolved_cursor_file(), &cursor)?;
        }
        if options.once {
            break;
        }
        sleep(Duration::from_secs(options.poll_interval_secs.max(1))).await;
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn watch_source(
    options: KapiWatchOptions,
    tx: mpsc::Sender<IncomingEvent>,
) -> Result<()> {
    validate_options(&options)?;
    loop {
        let events = poll_kapi_events(&options).await?;
        for event in &events.events {
            tx.send(event.clone()).await?;
        }
        if let Some(cursor) = events.cursor {
            write_cursor(&options.resolved_cursor_file(), &cursor)?;
        }
        if options.once {
            break;
        }
        sleep(Duration::from_secs(options.poll_interval_secs.max(1))).await;
    }
    Ok(())
}

fn validate_options(options: &KapiWatchOptions) -> Result<()> {
    if options.require_discord_topic && options.discord_topic_id.is_none() {
        return Err("--require-discord-topic requires --discord-topic-id".into());
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct KapiPollResult {
    pub events: Vec<IncomingEvent>,
    pub cursor: Option<String>,
}

async fn poll_kapi_events(options: &KapiWatchOptions) -> Result<KapiPollResult> {
    let since = read_cursor(&options.resolved_cursor_file())?;
    let mut command = Command::new("kapi");
    command
        .arg("events")
        .arg("--from")
        .arg(&options.from)
        .arg("--json");
    if let Some(cursor) = since.as_deref().filter(|cursor| !cursor.is_empty()) {
        command.arg("--since").arg(cursor);
    }

    let output = command.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(format!(
            "kapi events failed with status {}: {}{}",
            output.status,
            stderr,
            if stdout.is_empty() {
                String::new()
            } else {
                format!("\nstdout: {stdout}")
            }
        )
        .into());
    }

    let value: Value = serde_json::from_slice(&output.stdout)?;
    parse_kapi_events(value, options)
}

pub fn parse_kapi_events(value: Value, options: &KapiWatchOptions) -> Result<KapiPollResult> {
    let raw_events: Vec<Value> = match value {
        Value::Array(events) => events,
        Value::Object(mut object) => object
            .remove("events")
            .or_else(|| object.remove("items"))
            .and_then(|events| events.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let mut cursor = None;
    let mut events = Vec::new();
    for raw in raw_events {
        let Some(event) = event_from_kapi_value(raw, options)? else {
            continue;
        };
        if let Some(next_cursor) = event
            .payload
            .get("cursor")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            cursor = Some(next_cursor);
        }
        events.push(event);
    }

    Ok(KapiPollResult { events, cursor })
}

fn event_from_kapi_value(raw: Value, options: &KapiWatchOptions) -> Result<Option<IncomingEvent>> {
    let Value::Object(mut object) = raw else {
        return Ok(None);
    };

    let event_type = object
        .remove("type")
        .or_else(|| object.remove("kind"))
        .or_else(|| object.remove("event"))
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or("kapi event is missing type")?;

    if !event_type.starts_with("kapi.worker.") {
        return Ok(None);
    }

    let payload = object
        .remove("payload")
        .and_then(|payload| payload.as_object().cloned())
        .unwrap_or_else(|| object.clone());
    let mut payload = payload;

    payload
        .entry("repo")
        .or_insert_with(|| json!(options.from.display().to_string()));
    payload
        .entry("stale_minutes")
        .or_insert_with(|| json!(options.stale_minutes));
    if let Some(topic) = &options.discord_topic_id {
        payload
            .entry("discord_topic_id")
            .or_insert_with(|| json!(topic));
    }

    Ok(Some(IncomingEvent {
        kind: event_type,
        channel: options.channel.clone(),
        mention: mention_for_event(&payload, options),
        format: Some(options.format.clone()),
        template: None,
        payload: Value::Object(payload),
    }))
}

fn mention_for_event(payload: &Map<String, Value>, options: &KapiWatchOptions) -> Option<String> {
    let needs_followup = payload
        .get("recommended_action")
        .and_then(Value::as_str)
        .map(|action| !action.trim().is_empty())
        .unwrap_or(false);
    if needs_followup {
        options.mention.clone()
    } else {
        None
    }
}

fn read_cursor(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(cursor) => Ok(Some(cursor.trim().to_string()).filter(|cursor| !cursor.is_empty())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_cursor(path: &Path, cursor: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, cursor)?;
    Ok(())
}

fn default_cursor_dir() -> PathBuf {
    std::env::var_os("CLAWHIP_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".clawhip")))
        .unwrap_or_else(|| PathBuf::from(".clawhip"))
        .join("kapi")
        .join("cursors")
}

fn sanitize_cursor_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> KapiWatchOptions {
        KapiWatchOptions {
            from: PathBuf::from("/repo/kapi"),
            channel: Some("alerts".into()),
            mention: Some("<@123>".into()),
            stale_minutes: 5,
            format: MessageFormat::Compact,
            cursor_file: None,
            poll_interval_secs: 1,
            once: true,
            discord_topic_id: Some("456".into()),
            require_discord_topic: false,
        }
    }

    #[test]
    fn parses_kapi_worker_events_and_preserves_cursor() {
        let result = parse_kapi_events(
            json!({
                "events": [{
                    "type": "kapi.worker.review-ready",
                    "payload": {
                        "repo_name": "devkade/kapi",
                        "slug": "issue-58",
                        "status": "review-ready",
                        "reason": "terminal marker matched",
                        "recommended_action": "kapi report issue-58 --from /repo/kapi --json",
                        "cursor": "abc123"
                    }
                }]
            }),
            &options(),
        )
        .expect("parse result");

        assert_eq!(result.cursor.as_deref(), Some("abc123"));
        assert_eq!(result.events.len(), 1);
        let event = &result.events[0];
        assert_eq!(event.kind, "kapi.worker.review-ready");
        assert_eq!(event.channel.as_deref(), Some("alerts"));
        assert_eq!(event.mention.as_deref(), Some("<@123>"));
        assert_eq!(event.payload["repo_name"], json!("devkade/kapi"));
        assert_eq!(event.payload["discord_topic_id"], json!("456"));
    }

    #[test]
    fn filters_non_kapi_worker_events() {
        let result = parse_kapi_events(
            json!([
                {"type": "kapi.worker.started", "payload": {"cursor": "one"}},
                {"type": "tmux.keyword", "payload": {"cursor": "two"}}
            ]),
            &options(),
        )
        .expect("parse result");

        assert_eq!(result.events.len(), 1);
        assert_eq!(result.cursor.as_deref(), Some("one"));
    }

    #[test]
    fn require_topic_fails_fast_without_topic() {
        let mut options = options();
        options.discord_topic_id = None;
        options.require_discord_topic = true;

        let error = validate_options(&options).expect_err("topic should be required");
        assert!(error.to_string().contains("--require-discord-topic"));
    }
}
