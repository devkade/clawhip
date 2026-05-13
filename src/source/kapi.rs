use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::Result;
use crate::config::{AppConfig, KapiRepoMonitor};
use crate::events::{IncomingEvent, MessageFormat};
use crate::source::Source;

pub struct KapiSource {
    config: Arc<AppConfig>,
}

impl KapiSource {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self { config }
    }
}

#[async_trait::async_trait]
impl Source for KapiSource {
    fn name(&self) -> &str {
        "kapi"
    }

    async fn run(&self, tx: mpsc::Sender<IncomingEvent>) -> Result<()> {
        loop {
            for monitor in &self.config.monitors.kapi.repos {
                if let Err(error) = poll_and_send_monitor(monitor, Some(&tx)).await {
                    eprintln!(
                        "clawhip source kapi polling failed for {}: {error}",
                        monitor.from
                    );
                }
            }

            sleep(Duration::from_secs(
                self.config.monitors.poll_interval_secs.max(1),
            ))
            .await;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollOutcome {
    pub event_count: usize,
    pub cursor: Option<String>,
    pub cursor_path: PathBuf,
}

pub async fn poll_and_send_monitor(
    monitor: &KapiRepoMonitor,
    tx: Option<&mpsc::Sender<IncomingEvent>>,
) -> Result<PollOutcome> {
    let (events, outcome) = poll_monitor_events(monitor).await?;
    if let Some(tx) = tx {
        for event in events {
            tx.send(event)
                .await
                .map_err(|error| format!("kapi source channel closed: {error}"))?;
        }
    }
    Ok(outcome)
}

pub async fn poll_monitor_events(
    monitor: &KapiRepoMonitor,
) -> Result<(Vec<IncomingEvent>, PollOutcome)> {
    let cursor_path = cursor_path_for_monitor(monitor)?;
    let since = read_cursor(&cursor_path)?;
    let raw = run_kapi_events(monitor, since.as_deref()).await?;
    let parsed = parse_kapi_events_output(&raw)?;
    let mut next_cursor = parsed.cursor.clone();
    let mut events = Vec::new();

    for wire in parsed.events {
        let event = kapi_wire_event_to_incoming(wire, monitor)?;
        next_cursor = event_cursor(&event).or_else(|| next_cursor.clone());
        events.push(event);
    }

    if let Some(cursor) = next_cursor.as_deref() {
        write_cursor(&cursor_path, cursor)?;
    }

    let event_count = events.len();
    Ok((
        events,
        PollOutcome {
            event_count,
            cursor: next_cursor,
            cursor_path,
        },
    ))
}

async fn run_kapi_events(monitor: &KapiRepoMonitor, since: Option<&str>) -> Result<String> {
    let mut command = Command::new(kapi_bin(monitor));
    command
        .arg("events")
        .arg("--from")
        .arg(&monitor.from)
        .arg("--json");
    if let Some(since) = since.filter(|value| !value.trim().is_empty()) {
        command.arg("--since").arg(since);
    }

    let output = command.output().await?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    } else {
        Err(format!(
            "kapi events failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into())
    }
}

fn kapi_bin(monitor: &KapiRepoMonitor) -> String {
    monitor
        .kapi_bin
        .clone()
        .or_else(|| std::env::var("CLAWHIP_KAPI_BIN").ok())
        .unwrap_or_else(|| "kapi".to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedKapiEvents {
    pub events: Vec<KapiWireEvent>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KapiWireEvent {
    pub kind: String,
    pub channel: Option<String>,
    pub mention: Option<String>,
    pub format: Option<MessageFormat>,
    pub payload: Value,
}

pub fn parse_kapi_events_output(raw: &str) -> Result<ParsedKapiEvents> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(ParsedKapiEvents {
            events: Vec::new(),
            cursor: None,
        });
    }

    let value: Value = serde_json::from_str(trimmed)?;
    match value {
        Value::Array(values) => Ok(ParsedKapiEvents {
            events: values
                .into_iter()
                .map(kapi_wire_event_from_value)
                .collect::<Result<Vec<_>>>()?,
            cursor: None,
        }),
        Value::Object(mut object) => {
            let cursor = string_from_object(&object, &["cursor", "next_cursor", "nextCursor"]);
            let events_value = object
                .remove("events")
                .or_else(|| object.remove("items"))
                .or_else(|| object.remove("data"));
            let events = match events_value {
                Some(Value::Array(values)) => values
                    .into_iter()
                    .map(kapi_wire_event_from_value)
                    .collect::<Result<Vec<_>>>()?,
                Some(other) => vec![kapi_wire_event_from_value(other)?],
                None if object.get("type").is_some()
                    || object.get("kind").is_some()
                    || object.get("event").is_some() =>
                {
                    vec![kapi_wire_event_from_value(Value::Object(object))?]
                }
                None => Vec::new(),
            };
            Ok(ParsedKapiEvents { events, cursor })
        }
        other => Err(format!("unsupported kapi events JSON shape: {other}").into()),
    }
}

fn kapi_wire_event_from_value(value: Value) -> Result<KapiWireEvent> {
    let Value::Object(mut object) = value else {
        return Err("kapi event entries must be JSON objects".into());
    };

    let kind = take_string(&mut object, &["type", "kind", "event"])
        .unwrap_or_else(|| "kapi.worker.output-changed".to_string());
    let channel = take_string(&mut object, &["channel"]);
    let mention = take_string(&mut object, &["mention"]);
    let format = take_string(&mut object, &["format"])
        .map(|value| MessageFormat::from_label(&value))
        .transpose()?;
    let payload = match object.remove("payload") {
        Some(Value::Object(mut payload)) => {
            payload.extend(object);
            Value::Object(payload)
        }
        Some(payload) if object.is_empty() => payload,
        Some(payload) => {
            let mut merged = Map::new();
            merged.insert("value".to_string(), payload);
            merged.extend(object);
            Value::Object(merged)
        }
        None => Value::Object(object),
    };

    Ok(KapiWireEvent {
        kind,
        channel,
        mention,
        format,
        payload,
    })
}

pub fn kapi_wire_event_to_incoming(
    wire: KapiWireEvent,
    monitor: &KapiRepoMonitor,
) -> Result<IncomingEvent> {
    let mut payload = ensure_object_payload(wire.payload);
    insert_string_if_missing(&mut payload, "repo", Some(monitor.from.clone()));
    insert_string_if_missing(&mut payload, "source", Some("kapi".to_string()));
    insert_string_if_missing(
        &mut payload,
        "stale_minutes",
        Some(monitor.stale_minutes.to_string()),
    );

    let event_channel = wire
        .channel
        .or_else(|| string_field(&payload, "discord_topic_id"))
        .or_else(|| string_field(&payload, "channel"))
        .or_else(|| monitor.channel.clone());
    let event_mention = wire.mention.or_else(|| monitor.mention.clone());
    let event_format = wire.format.or_else(|| monitor.format.clone());

    Ok(IncomingEvent {
        kind: wire.kind,
        channel: event_channel,
        mention: event_mention,
        format: event_format,
        template: None,
        payload,
    })
}

fn ensure_object_payload(value: Value) -> Value {
    match value {
        Value::Object(_) => value,
        other => json!({ "value": other }),
    }
}

fn insert_string_if_missing(payload: &mut Value, key: &str, value: Option<String>) {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return;
    };
    if let Some(object) = payload.as_object_mut() {
        object
            .entry(key.to_string())
            .or_insert_with(|| json!(value));
    }
}

fn event_cursor(event: &IncomingEvent) -> Option<String> {
    string_field(&event.payload, "cursor")
        .or_else(|| string_field(&event.payload, "event_id"))
        .or_else(|| string_field(&event.payload, "id"))
}

fn read_cursor(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_string()).filter(|value| !value.is_empty())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_cursor(path: &Path, cursor: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", cursor.trim()))?;
    Ok(())
}

pub fn cursor_path_for_monitor(monitor: &KapiRepoMonitor) -> Result<PathBuf> {
    if let Some(path) = &monitor.cursor_path {
        return Ok(PathBuf::from(path));
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Ok(PathBuf::from(home)
        .join(".clawhip")
        .join("kapi-cursors")
        .join(format!("{}.cursor", stable_monitor_key(&monitor.from))))
}

fn stable_monitor_key(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    let hash = hasher.finish();
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>();
    let slug = if slug.is_empty() { "repo" } else { &slug };
    format!("{slug}-{hash:016x}")
}

fn take_string(object: &mut Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = object.remove(*key).and_then(value_to_string) {
            return Some(value);
        }
    }
    None
}

fn string_from_object(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_to_string_ref))
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(value_to_string_ref)
}

fn value_to_string(value: Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
    .filter(|value| !value.trim().is_empty())
}

fn value_to_string_ref(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
    .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor() -> KapiRepoMonitor {
        KapiRepoMonitor {
            from: "/repo/kapi".into(),
            channel: Some("alerts".into()),
            mention: Some("<@hermes>".into()),
            stale_minutes: 5,
            format: Some(MessageFormat::Compact),
            cursor_path: None,
            kapi_bin: None,
        }
    }

    #[test]
    fn parses_array_event_output() {
        let parsed = parse_kapi_events_output(
            r#"[{"type":"kapi.worker.review-ready","payload":{"slug":"issue-2","cursor":"c1"}}]"#,
        )
        .expect("parsed");

        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.events[0].kind, "kapi.worker.review-ready");
        assert_eq!(parsed.events[0].payload["slug"], "issue-2");
    }

    #[test]
    fn parses_object_with_events_and_cursor() {
        let parsed = parse_kapi_events_output(
            r#"{"cursor":"c2","events":[{"type":"kapi.worker.blocked","slug":"issue-2","reason":"needs review"}]}"#,
        )
        .expect("parsed");

        assert_eq!(parsed.cursor.as_deref(), Some("c2"));
        assert_eq!(parsed.events[0].payload["reason"], "needs review");
    }

    #[test]
    fn converts_wire_event_with_monitor_defaults_and_topic_override() {
        let event = kapi_wire_event_to_incoming(
            KapiWireEvent {
                kind: "kapi.worker.review-ready".into(),
                channel: None,
                mention: None,
                format: None,
                payload: json!({
                    "slug": "issue-2",
                    "status": "review-ready",
                    "discord_topic_id": "thread-123",
                    "recommended_action": "kapi report issue-2 --json"
                }),
            },
            &monitor(),
        )
        .expect("event");

        assert_eq!(event.kind, "kapi.worker.review-ready");
        assert_eq!(event.channel.as_deref(), Some("thread-123"));
        assert_eq!(event.mention.as_deref(), Some("<@hermes>"));
        assert_eq!(event.format, Some(MessageFormat::Compact));
        assert_eq!(event.payload["repo"], "/repo/kapi");
    }
}
