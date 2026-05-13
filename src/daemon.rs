use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router as AxumRouter};
use serde_json::{Value, json};
use tokio::sync::{RwLock, mpsc};

use crate::Result;
use crate::VERSION;
use crate::config::AppConfig;
use crate::dispatch::Dispatcher;
use crate::event::compat::from_incoming_event;
use crate::events::{IncomingEvent, MessageFormat, normalize_event};
use crate::pi_state::{PiActivity, PiConfidence, PiLifecycle, PiSessionState, unix_now};
use crate::pi_state_store::{SharedPiStateStore, new_shared_pi_state_store};
use crate::render::{DefaultRenderer, Renderer};
use crate::router::Router;
use crate::sink::{DiscordSink, Sink, SlackSink};
use crate::source::{
    GitHubSource, GitSource, RegisteredTmuxSession, SharedTmuxRegistry, Source, TmuxSource,
};

const EVENT_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    port: u16,
    tx: mpsc::Sender<IncomingEvent>,
    tmux_registry: SharedTmuxRegistry,
    #[allow(dead_code)]
    pi_state_store: SharedPiStateStore,
}

pub async fn run(config: Arc<AppConfig>, port_override: Option<u16>) -> Result<()> {
    config.validate()?;
    let token_source = config.discord_token_source();
    println!("clawhip v{VERSION} starting (token_source: {token_source})");

    let mut sinks: HashMap<String, Box<dyn Sink>> = HashMap::new();
    sinks.insert(
        "discord".into(),
        Box::new(DiscordSink::from_config(config.clone())?),
    );
    sinks.insert("slack".into(), Box::new(SlackSink::default()));
    let renderer: Box<dyn Renderer> = Box::new(DefaultRenderer);
    let router = Router::new(config.clone());
    let tmux_registry: SharedTmuxRegistry = Arc::new(RwLock::new(HashMap::new()));
    let pi_state_store: SharedPiStateStore = new_shared_pi_state_store();
    let (tx, rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);

    tokio::spawn(async move {
        let mut dispatcher = Dispatcher::new(rx, router, renderer, sinks);
        if let Err(error) = dispatcher.run().await {
            eprintln!("clawhip dispatcher stopped: {error}");
        }
    });
    spawn_source(GitSource::new(config.clone()), tx.clone());
    spawn_source(GitHubSource::new(config.clone()), tx.clone());
    spawn_source(
        TmuxSource::new(
            config.clone(),
            tmux_registry.clone(),
            pi_state_store.clone(),
        ),
        tx.clone(),
    );

    let app = AxumRouter::new()
        .route("/health", get(health))
        .route("/api/status", get(status))
        .route("/api/pi/state", get(pi_state_index))
        .route("/api/pi/state/{session}", get(pi_state_show))
        .route("/api/pi/render-summary", get(pi_render_summary_compact))
        .route("/api/pi/render-summary/{format}", get(pi_render_summary))
        .route("/event", post(post_event))
        .route("/api/event", post(post_event))
        .route("/events", post(post_event))
        .route("/api/tmux/register", post(register_tmux))
        .route("/github", post(post_github));
    let port = port_override.unwrap_or(config.daemon.port);

    let app = app.with_state(AppState {
        config: config.clone(),
        port,
        tx,
        tmux_registry,
        pi_state_store,
    });
    let addr: SocketAddr = format!("{}:{}", config.daemon.bind_host, port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!(
        "clawhip daemon v{VERSION} listening on http://{} (token_source: {token_source})",
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;
    Ok(())
}

fn spawn_source<S>(source: S, tx: mpsc::Sender<IncomingEvent>)
where
    S: Source + Send + Sync + 'static,
{
    tokio::spawn(async move {
        println!("clawhip source '{}' starting", source.name());
        if let Err(error) = source.run(tx).await {
            eprintln!("clawhip source '{}' stopped: {error}", source.name());
        }
    });
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let registered = state.tmux_registry.read().await.len();
    Json(health_payload(
        state.config.as_ref(),
        state.port,
        registered,
    ))
}

fn health_payload(config: &AppConfig, port: u16, registered_tmux_sessions: usize) -> Value {
    json!({
        "ok": true,
        "version": VERSION,
        "token_source": config.discord_token_source(),
        "webhook_routes_configured": config.has_webhook_routes(),
        "port": port,
        "daemon_base_url": config.daemon.base_url,
        "configured_git_monitors": config.monitors.git.repos.len(),
        "configured_tmux_monitors": config.monitors.tmux.sessions.len(),
        "registered_tmux_sessions": registered_tmux_sessions,
    })
}

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    health(State(state)).await
}

fn build_pi_state_index_payload(mut sessions: Vec<PiSessionState>) -> Value {
    sessions.sort_by(|a, b| a.session_name.cmp(&b.session_name));

    let mut lifecycle_counts: HashMap<String, usize> = HashMap::new();
    let mut activity_counts: HashMap<String, usize> = HashMap::new();
    let mut stale_sessions = Vec::new();
    let mut blocked_sessions = Vec::new();
    let mut running_sessions = Vec::new();
    let mut tool_error_sessions = Vec::new();
    let mut tool_hint_counts: HashMap<String, usize> = HashMap::new();

    for session in &sessions {
        *lifecycle_counts
            .entry(format!("{:?}", session.lifecycle).to_ascii_lowercase())
            .or_insert(0) += 1;
        *activity_counts
            .entry(format!("{:?}", session.activity).to_ascii_lowercase())
            .or_insert(0) += 1;

        if session.stale {
            stale_sessions.push(session.session_name.clone());
        }
        if matches!(session.activity, PiActivity::BlockedOrWaiting) {
            blocked_sessions.push(session.session_name.clone());
        }
        if matches!(session.lifecycle, PiLifecycle::Running) {
            running_sessions.push(session.session_name.clone());
        }
        if session.last_tool_error.is_some() {
            tool_error_sessions.push(session.session_name.clone());
        }
        if let Some(hint) = &session.last_tool_hint {
            *tool_hint_counts.entry(hint.clone()).or_insert(0) += 1;
        }
    }

    let active_cycle_sessions: Vec<String> = sessions
        .iter()
        .filter(|session| session.cycle_active)
        .map(|session| session.session_name.clone())
        .collect();
    let tool_active_sessions: Vec<String> = sessions
        .iter()
        .filter(|session| session.tool_active)
        .map(|session| session.session_name.clone())
        .collect();

    json!({
        "ok": true,
        "count": sessions.len(),
        "summary": {
            "lifecycle_counts": lifecycle_counts,
            "activity_counts": activity_counts,
            "stale_sessions": stale_sessions,
            "blocked_sessions": blocked_sessions,
            "running_sessions": running_sessions,
            "active_cycle_sessions": active_cycle_sessions,
            "tool_active_count": tool_active_sessions.len(),
            "tool_active_sessions": tool_active_sessions,
            "tool_error_sessions": tool_error_sessions,
            "tool_hint_counts": tool_hint_counts,
        },
        "sessions": sessions,
    })
}

async fn pi_state_index(State(state): State<AppState>) -> impl IntoResponse {
    let read = state.pi_state_store.read().await;
    let sessions: Vec<_> = read.values().cloned().collect();
    Json(build_pi_state_index_payload(sessions))
}

async fn pi_state_show(
    State(state): State<AppState>,
    Path(session): Path<String>,
) -> impl IntoResponse {
    let read = state.pi_state_store.read().await;
    match read.get(&session) {
        Some(pi_state) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "summary": {
                    "session": pi_state.session_name,
                    "lifecycle": format!("{:?}", pi_state.lifecycle).to_ascii_lowercase(),
                    "activity": format!("{:?}", pi_state.activity).to_ascii_lowercase(),
                    "stale": pi_state.stale,
                    "attachable": pi_state.attachable,
                },
                "session_state": pi_state,
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": format!("pi session not found: {session}")
            })),
        )
            .into_response(),
    }
}

async fn pi_render_summary_compact(State(state): State<AppState>) -> impl IntoResponse {
    render_pi_summary_response(&state, MessageFormat::Compact)
        .await
        .into_response()
}

async fn pi_render_summary(
    State(state): State<AppState>,
    Path(format): Path<String>,
) -> impl IntoResponse {
    match MessageFormat::from_label(&format) {
        Ok(format) => render_pi_summary_response(&state, format)
            .await
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn render_pi_summary_response(
    state: &AppState,
    format: MessageFormat,
) -> (StatusCode, Json<Value>) {
    let read = state.pi_state_store.read().await;
    let payload = build_pi_state_index_payload(read.values().cloned().collect());
    let renderer = DefaultRenderer;
    let event = IncomingEvent {
        kind: "pi.state-summary".into(),
        channel: None,
        mention: None,
        format: Some(format.clone()),
        template: None,
        payload: payload.clone(),
    };

    match renderer.render(&event, &format) {
        Ok(rendered) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "format": format.as_str(),
                "rendered": rendered,
                "payload": payload,
            })),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": error.to_string()})),
        ),
    }
}

async fn post_event(
    State(state): State<AppState>,
    Json(event): Json<IncomingEvent>,
) -> impl IntoResponse {
    let event = normalize_event(event);
    apply_pi_wrapper_event_to_state(&state.pi_state_store, &event).await;
    if let Err(error) = from_incoming_event(&event) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response();
    }

    match enqueue_event(&state.tx, event.clone()).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({"ok": true, "type": event.kind})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn register_tmux(
    State(state): State<AppState>,
    Json(registration): Json<RegisteredTmuxSession>,
) -> impl IntoResponse {
    state
        .tmux_registry
        .write()
        .await
        .insert(registration.session.clone(), registration.clone());
    (
        StatusCode::ACCEPTED,
        Json(json!({"ok": true, "session": registration.session})),
    )
        .into_response()
}

async fn post_github(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let event_name = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let action = payload
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let event = match event_name {
        "issues" if action == "opened" => {
            Some(normalize_event(IncomingEvent::github_issue_opened(
                payload
                    .pointer("/repository/full_name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown/unknown")
                    .to_string(),
                payload
                    .pointer("/issue/number")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                payload
                    .pointer("/issue/title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled issue")
                    .to_string(),
                None,
            )))
        }
        "pull_request" => {
            let repo = payload
                .pointer("/repository/full_name")
                .and_then(Value::as_str)
                .unwrap_or("unknown/unknown")
                .to_string();
            let number = payload
                .pointer("/pull_request/number")
                .or_else(|| payload.pointer("/number"))
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let title = payload
                .pointer("/pull_request/title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled pull request")
                .to_string();
            let url = payload
                .pointer("/pull_request/html_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match action {
                "opened" => Some(normalize_event(IncomingEvent::github_pr_status_changed(
                    repo,
                    number,
                    title,
                    "unknown".to_string(),
                    "opened".to_string(),
                    url,
                    None,
                ))),
                "closed" => Some(normalize_event(IncomingEvent::github_pr_status_changed(
                    repo,
                    number,
                    title,
                    "open".to_string(),
                    "closed".to_string(),
                    url,
                    None,
                ))),
                _ => None,
            }
        }
        _ => None,
    };

    let Some(event) = event else {
        let reason = if event_name == "pull_request" {
            "unsupported pull_request action"
        } else {
            "unsupported event"
        };
        return (
            StatusCode::ACCEPTED,
            Json(json!({"ok": true, "ignored": true, "reason": reason})),
        )
            .into_response();
    };

    if let Err(error) = from_incoming_event(&event) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response();
    }

    match enqueue_event(&state.tx, event).await {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn apply_pi_wrapper_event_to_state(store: &SharedPiStateStore, event: &IncomingEvent) {
    if event.payload.get("tool").and_then(Value::as_str) != Some("pi") {
        return;
    }

    let kind = event.canonical_kind();
    if !matches!(
        kind,
        "session.started" | "session.finished" | "session.failed"
    ) {
        return;
    }

    let session_name = event
        .payload
        .get("session_name")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            event
                .payload
                .get("session_id")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        });
    let Some(session_name) = session_name else {
        return;
    };

    let project = event
        .payload
        .get("project")
        .or_else(|| event.payload.get("repo_name"))
        .and_then(Value::as_str)
        .unwrap_or(&session_name)
        .to_string();
    let repo_path = event
        .payload
        .get("repo_path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let branch = event
        .payload
        .get("branch")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let now = unix_now();

    let mut write = store.write().await;
    let state = write.entry(session_name.clone()).or_insert_with(|| {
        PiSessionState::new(
            session_name.clone(),
            project.clone(),
            repo_path.clone(),
            session_name.clone(),
            branch.clone(),
        )
    });
    state.sources.wrapper_emit = true;

    match kind {
        "session.started" => {
            state.mark_running(now);
            state.confidence.lifecycle = PiConfidence::High;
        }
        "session.finished" => {
            state.mark_finished(now, Some(0));
            state.confidence.lifecycle = PiConfidence::High;
        }
        "session.failed" => {
            state.mark_failed(
                now,
                event
                    .payload
                    .get("error_message")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or_else(|| {
                        event
                            .payload
                            .get("summary")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    }),
                None,
            );
            state.confidence.lifecycle = PiConfidence::High;
        }
        _ => {}
    }
}

async fn enqueue_event(tx: &mpsc::Sender<IncomingEvent>, event: IncomingEvent) -> Result<()> {
    tx.send(event)
        .await
        .map_err(|error| format!("event queue unavailable: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn health_payload_includes_version_and_token_source() {
        let mut config = AppConfig::default();
        config.providers.discord.bot_token = Some("config-token".into());
        config.monitors.git.repos.push(Default::default());
        config.monitors.tmux.sessions.push(Default::default());

        let payload = health_payload(&config, 25294, 3);

        assert_eq!(payload["ok"], Value::Bool(true));
        assert_eq!(payload["version"], Value::String(VERSION.to_string()));
        assert_eq!(payload["token_source"], Value::String("config".to_string()));
        assert_eq!(payload["port"], Value::from(25294));
        assert_eq!(payload["configured_git_monitors"], Value::from(1));
        assert_eq!(payload["configured_tmux_monitors"], Value::from(1));
        assert_eq!(payload["registered_tmux_sessions"], Value::from(3));
    }

    #[tokio::test]
    async fn apply_pi_wrapper_event_updates_state_store() {
        let store = new_shared_pi_state_store();
        let event = normalize_event(IncomingEvent {
            kind: "session.failed".into(),
            channel: None,
            mention: None,
            format: None,
            template: None,
            payload: json!({
                "tool": "pi",
                "session_name": "issue-9",
                "project": "repo",
                "repo_path": "/repo",
                "error_message": "boom",
            }),
        });

        apply_pi_wrapper_event_to_state(&store, &event).await;

        let read = store.read().await;
        let state = read.get("issue-9").expect("state present");
        assert_eq!(state.tool, "pi");
        assert_eq!(state.project, "repo");
        assert_eq!(state.failure_reason.as_deref(), Some("boom"));
        assert!(state.sources.wrapper_emit);
    }

    #[tokio::test]
    async fn pi_state_index_includes_summary_counts() {
        let store = new_shared_pi_state_store();
        {
            let mut write = store.write().await;
            let mut running = PiSessionState::new(
                "issue-1".into(),
                "repo".into(),
                "/repo".into(),
                "issue-1".into(),
                None,
            );
            running.mark_running(100);
            running.mark_pane_change(105, "working".into());
            running.set_tool_state(true, Some("cargo test".into()), None, 106);
            write.insert(running.session_name.clone(), running);

            let mut blocked = PiSessionState::new(
                "issue-3".into(),
                "repo".into(),
                "/repo".into(),
                "issue-3".into(),
                None,
            );
            blocked.mark_running(106);
            blocked.mark_blocked_or_waiting(110);
            write.insert(blocked.session_name.clone(), blocked);

            let mut failed = PiSessionState::new(
                "issue-2".into(),
                "repo".into(),
                "/repo".into(),
                "issue-2".into(),
                None,
            );
            failed.mark_failed(120, Some("boom".into()), None);
            failed.mark_stale(130);
            failed.set_tool_state(false, None, Some("boom".into()), 131);
            write.insert(failed.session_name.clone(), failed);
        }

        let read = store.read().await;
        let payload = build_pi_state_index_payload(read.values().cloned().collect());
        assert_eq!(payload["count"], Value::from(3));
        assert_eq!(
            payload["summary"]["lifecycle_counts"]["running"],
            Value::from(2)
        );
        assert_eq!(
            payload["summary"]["lifecycle_counts"]["failed"],
            Value::from(1)
        );
        assert_eq!(
            payload["summary"]["blocked_sessions"][0],
            Value::from("issue-3")
        );
        assert_eq!(
            payload["summary"]["stale_sessions"][0],
            Value::from("issue-2")
        );
        assert_eq!(
            payload["summary"]["active_cycle_sessions"][0],
            Value::from("issue-1")
        );
        assert_eq!(
            payload["summary"]["tool_active_sessions"][0],
            Value::from("issue-1")
        );
        assert_eq!(payload["summary"]["tool_active_count"], Value::from(1));
        assert_eq!(
            payload["summary"]["tool_hint_counts"]["cargo test"],
            Value::from(1)
        );
        assert_eq!(
            payload["summary"]["tool_error_sessions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
