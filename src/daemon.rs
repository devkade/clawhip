use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
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
use crate::cron::CronSource;
use crate::dispatch::Dispatcher;
use crate::event::compat::from_incoming_event;
use crate::events::{IncomingEvent, MessageFormat, normalize_event};
use crate::native_hooks::incoming_event_from_native_hook_json;
use crate::pi_state::{PiActivity, PiConfidence, PiLifecycle, PiSessionState, unix_now};
use crate::pi_state_store::{SharedPiStateStore, new_shared_pi_state_store};
use crate::render::{DefaultRenderer, Renderer};
use crate::router::Router;
use crate::sink::{DiscordSink, Sink, SlackSink};
use crate::source::{
    GitHubSource, GitSource, RegisteredTmuxSession, SharedTmuxRegistry, Source, TmuxSource,
    WorkspaceSource, list_active_tmux_registrations,
};
use crate::update::{self, SharedPendingUpdate};

const EVENT_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    port: u16,
    tx: mpsc::Sender<IncomingEvent>,
    tmux_registry: SharedTmuxRegistry,
    pi_state_store: SharedPiStateStore,
    pending_update: SharedPendingUpdate,
}

pub async fn run(
    config: Arc<AppConfig>,
    port_override: Option<u16>,
    cron_state_path: PathBuf,
) -> Result<()> {
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

    let ci_batch_window = config.dispatch.ci_batch_window();
    let routine_batch_window = config.dispatch.routine_batch_window();
    tokio::spawn(async move {
        let mut dispatcher = Dispatcher::new(
            rx,
            router,
            renderer,
            sinks,
            ci_batch_window,
            routine_batch_window,
        );
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
    spawn_source(WorkspaceSource::new(config.clone()), tx.clone());
    spawn_source(CronSource::new(config.clone(), cron_state_path), tx.clone());

    let pending_update = update::new_shared_pending_update();
    {
        let config = config.clone();
        let tx = tx.clone();
        let pending = pending_update.clone();
        tokio::spawn(async move {
            update::run_checker(config, tx, pending).await;
        });
    }

    let app = AxumRouter::new()
        .route("/health", get(health))
        .route("/api/status", get(status))
        .route("/event", post(post_event))
        .route("/api/event", post(post_event))
        .route("/events", post(post_event))
        .route("/native/hook", post(post_native_hook))
        .route("/api/native/hook", post(post_native_hook))
        .route("/api/tmux/register", post(register_tmux))
        .route("/api/tmux", get(list_tmux))
        .route("/api/pi/state", get(pi_state_index))
        .route("/api/pi/state/{session}", get(pi_state_show))
        .route("/api/pi/render-summary", get(pi_render_summary_compact))
        .route("/api/pi/render-summary/{format}", get(pi_render_summary))
        .route("/github", post(post_github))
        .route("/api/update/status", get(update_status))
        .route("/api/update/approve", post(approve_update))
        .route("/api/update/dismiss", post(dismiss_update));
    let port = port_override.unwrap_or(config.daemon.port);

    let app = app.with_state(AppState {
        config: config.clone(),
        port,
        tx,
        tmux_registry,
        pi_state_store,
        pending_update,
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
    let source_name = source.name().to_string();
    tokio::spawn(async move {
        println!("clawhip source '{}' starting", source_name);
        if let Err(error) = source.run(tx.clone()).await {
            eprintln!("clawhip source '{}' stopped: {error}", source_name);
            if let Err(alert_error) = tx
                .send(source_failure_alert_event(&source_name, &error.to_string()))
                .await
            {
                eprintln!(
                    "clawhip source '{}' could not enqueue degraded alert: {alert_error}",
                    source_name
                );
            }
        }
    });
}

fn source_failure_alert_event(source_name: &str, error_message: &str) -> IncomingEvent {
    let mut event = IncomingEvent::custom(
        None,
        format!("clawhip degraded: source '{source_name}' stopped: {error_message}"),
    )
    .with_format(Some(MessageFormat::Alert));

    if let Some(payload) = event.payload.as_object_mut() {
        payload.insert("source_name".to_string(), json!(source_name));
        payload.insert("health_status".to_string(), json!("degraded"));
        payload.insert("error_message".to_string(), json!(error_message));
    }

    event
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
        "configured_workspace_monitors": config.monitors.workspace.len(),
        "configured_cron_jobs": config.cron.jobs.len(),
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
    accept_event(&state, normalize_event(event)).await
}

async fn post_native_hook(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let event = match incoming_event_from_native_hook_json(&payload) {
        Ok(event) => normalize_event(event),
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "error": error.to_string()})),
            )
                .into_response();
        }
    };

    accept_event(&state, event).await
}

async fn accept_event(state: &AppState, event: IncomingEvent) -> axum::response::Response {
    apply_pi_wrapper_event_to_state(&state.pi_state_store, &event).await;
    let envelope = match from_incoming_event(&event) {
        Ok(envelope) => envelope,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "error": error.to_string()})),
            )
                .into_response();
        }
    };

    match enqueue_event(&state.tx, event.clone()).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "ok": true,
                "type": event.kind,
                "event_id": envelope.id.to_string(),
            })),
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

async fn list_tmux(State(state): State<AppState>) -> impl IntoResponse {
    match list_active_tmux_registrations(state.config.as_ref(), &state.tmux_registry).await {
        Ok(registrations) => (StatusCode::OK, Json(json!(registrations))).into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response(),
    }
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
        "release" if matches!(action, "published" | "released" | "prereleased" | "edited") => {
            let repo = payload
                .pointer("/repository/full_name")
                .and_then(Value::as_str)
                .unwrap_or("unknown/unknown")
                .to_string();
            let tag = payload
                .pointer("/release/tag_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let name = payload
                .pointer("/release/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let is_prerelease = payload
                .pointer("/release/prerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let url = payload
                .pointer("/release/html_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let actor = payload
                .pointer("/sender/login")
                .and_then(Value::as_str)
                .map(ToString::to_string);

            Some(normalize_event(IncomingEvent::github_release(
                action,
                repo,
                tag,
                name,
                is_prerelease,
                url,
                actor,
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

async fn update_status(State(state): State<AppState>) -> impl IntoResponse {
    let pending = state.pending_update.read().await;
    match pending.as_ref() {
        Some(update) => (
            StatusCode::OK,
            Json(json!({
                "pending": true,
                "current_version": update.current_version,
                "latest_version": update.latest_version,
                "release_url": update.release_url,
                "detected_at": update.detected_at,
            })),
        )
            .into_response(),
        None => (
            StatusCode::OK,
            Json(json!({
                "pending": false,
                "current_version": VERSION,
            })),
        )
            .into_response(),
    }
}

async fn approve_update(State(state): State<AppState>) -> impl IntoResponse {
    match update::approve_update(&state.pending_update, &state.config, &state.tx).await {
        Ok(update) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "updated_to": update.latest_version,
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn dismiss_update(State(state): State<AppState>) -> impl IntoResponse {
    match update::dismiss_update(&state.pending_update).await {
        Ok(update) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "dismissed_version": update.latest_version,
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn enqueue_event(tx: &mpsc::Sender<IncomingEvent>, event: IncomingEvent) -> Result<()> {
    tx.send(event)
        .await
        .map_err(|error| format!("event queue unavailable: {error}").into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::config::{CronJob, CronJobKind};
    use crate::events::{MessageFormat, RoutingMetadata};
    use crate::router::Router;
    use crate::sink::SinkTarget;
    use crate::source::tmux::{ParentProcessInfo, RegistrationSource};
    use axum::body::to_bytes;
    use std::fs;
    use tempfile::tempdir;
    use tokio::time::{Duration, timeout};

    #[test]
    fn health_payload_includes_version_and_token_source() {
        let mut config = AppConfig::default();
        config.providers.discord.bot_token = Some("config-token".into());
        config.monitors.git.repos.push(Default::default());
        config.monitors.tmux.sessions.push(Default::default());
        config.monitors.workspace.push(Default::default());

        let payload = health_payload(&config, 25294, 3);

        assert_eq!(payload["ok"], Value::Bool(true));
        assert_eq!(payload["version"], Value::String(VERSION.to_string()));
        assert_eq!(payload["token_source"], Value::String("config".to_string()));
        assert_eq!(payload["port"], Value::from(25294));
        assert_eq!(payload["configured_git_monitors"], Value::from(1));
        assert_eq!(payload["configured_tmux_monitors"], Value::from(1));
        assert_eq!(payload["configured_workspace_monitors"], Value::from(1));
        assert_eq!(payload["registered_tmux_sessions"], Value::from(3));
    }

    #[tokio::test]
    async fn source_failure_alert_defaults_to_alert_format_and_default_channel_routing() {
        let event =
            source_failure_alert_event("cron", "EOF while parsing a value at line 1 column 0");

        assert_eq!(event.kind, "custom");
        assert_eq!(event.channel, None);
        assert_eq!(event.format, Some(MessageFormat::Alert));
        assert_eq!(event.payload["source_name"], Value::from("cron"));
        assert_eq!(event.payload["health_status"], Value::from("degraded"));
        assert!(
            event.payload["message"]
                .as_str()
                .is_some_and(|message| message.contains("source 'cron' stopped"))
        );

        let mut config = AppConfig::default();
        config.defaults.channel = Some("default-alerts".into());
        let router = Router::new(Arc::new(config));
        let delivery = router.preview_delivery(&event).await.expect("delivery");

        assert_eq!(
            delivery.target,
            SinkTarget::DiscordChannel("default-alerts".into())
        );
    }

    #[tokio::test]
    async fn spawn_source_allows_cron_source_to_start_with_empty_state_and_emit_job_event() {
        let dir = tempdir().expect("tempdir");
        let state_path = dir.path().join("cron-state.json");
        fs::write(&state_path, "").expect("write invalid cron state");

        let mut config = AppConfig::default();
        config.defaults.channel = Some("default-alerts".into());
        config.cron.jobs.push(CronJob {
            id: "dev-followup".into(),
            schedule: "* * * * *".into(),
            timezone: "UTC".into(),
            enabled: true,
            channel: Some("ops".into()),
            mention: None,
            format: Some(MessageFormat::Alert),
            state_file: None,
            kind: CronJobKind::CustomMessage {
                message: "check open PRs".into(),
            },
        });

        let (tx, mut rx) = mpsc::channel(4);
        spawn_source(CronSource::new(Arc::new(config.clone()), state_path), tx);

        let event = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for cron job event")
            .expect("cron job event");

        assert_eq!(event.kind, "custom");
        assert_eq!(event.channel, Some("ops".into()));
        assert_eq!(event.format, Some(MessageFormat::Alert));
        assert_eq!(event.payload["cron_job_id"], Value::from("dev-followup"));
        assert_eq!(event.payload["cron_timezone"], Value::from("UTC"));

        let router = Router::new(Arc::new(config));
        let delivery = router.preview_delivery(&event).await.expect("delivery");
        assert_eq!(delivery.target, SinkTarget::DiscordChannel("ops".into()));

        let rendered = router
            .render_delivery(&event, &delivery, &crate::render::DefaultRenderer)
            .await
            .expect("rendered event");
        assert!(rendered.contains("check open PRs"));
    }

    #[tokio::test]
    async fn post_event_returns_event_id_and_preserves_normalized_metadata() {
        let (tx, mut rx) = mpsc::channel(1);
        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: update::new_shared_pending_update(),
        };
        let event = IncomingEvent::agent_started(
            "worker-1".into(),
            Some("sess-123".into()),
            Some("my-repo".into()),
            None,
            Some("booted".into()),
            None,
            None,
        );

        let response = post_event(State(state), Json(event)).await.into_response();
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let response_json: Value = serde_json::from_slice(&body).unwrap();
        let event_id = response_json["event_id"].as_str().unwrap();
        assert!(!event_id.is_empty());
        assert_eq!(response_json["type"], Value::from("agent.started"));

        let queued = rx.recv().await.unwrap();
        assert_eq!(queued.payload["event_id"], Value::from(event_id));
        assert_eq!(queued.payload["correlation_id"], Value::from("sess-123"));
        assert!(
            queued
                .payload
                .get("first_seen_at")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        );
    }

    #[tokio::test]
    async fn post_native_hook_accepts_codex_payload_and_queues_normalized_event() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("clawhip");
        std::fs::create_dir_all(&repo).expect("create repo");
        let git = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo)
            .output()
            .expect("git init");
        assert!(
            git.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&git.stderr)
        );
        let (tx, mut rx) = mpsc::channel(1);
        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: update::new_shared_pending_update(),
        };
        let payload = json!({
            "provider": "codex",
            "event_name": "SessionStart",
            "directory": repo,
            "cwd": repo,
            "event_payload": {
                "session_id": "sess-65",
                "cwd": repo
            }
        });

        let response = post_native_hook(State(state), Json(payload))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let response_json: Value = serde_json::from_slice(&body).unwrap();
        let event_id = response_json["event_id"].as_str().unwrap();
        assert!(!event_id.is_empty());
        assert_eq!(response_json["type"], Value::from("session.started"));

        let queued = rx.recv().await.unwrap();
        assert_eq!(queued.kind, "session.started");
        assert_eq!(queued.payload["tool"], Value::from("codex"));
        assert_eq!(queued.payload["session_id"], Value::from("sess-65"));
        assert_eq!(queued.payload["event_id"], Value::from(event_id));
    }

    #[tokio::test]
    async fn post_native_hook_rejects_unsupported_event() {
        let (tx, _rx) = mpsc::channel(1);
        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: update::new_shared_pending_update(),
        };
        let payload = json!({
            "provider": "claude-code",
            "event_name": "Notification",
            "directory": "/repo/clawhip",
            "event_payload": {}
        });

        let response = post_native_hook(State(state), Json(payload))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let response_json: Value = serde_json::from_slice(&body).unwrap();
        assert!(
            response_json["error"]
                .as_str()
                .is_some_and(|error| error.contains("unsupported native hook event"))
        );
    }

    #[tokio::test]
    async fn list_tmux_returns_registered_sessions_with_metadata() {
        let (tx, _rx) = mpsc::channel(1);
        let registry: SharedTmuxRegistry = Arc::new(RwLock::new(HashMap::new()));
        registry.write().await.insert(
            "issue-105".into(),
            RegisteredTmuxSession {
                session: "issue-105".into(),
                channel: Some("alerts".into()),
                mention: Some("<@123>".into()),
                routing: RoutingMetadata::default(),
                keywords: vec!["error".into()],
                keyword_window_secs: 30,
                stale_minutes: 15,
                format: None,
                registered_at: "2026-04-02T00:00:00Z".into(),
                registration_source: RegistrationSource::CliWatch,
                parent_process: Some(ParentProcessInfo {
                    pid: 4242,
                    name: Some("codex".into()),
                }),
                active_wrapper_monitor: true,
            },
        );
        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: registry,
            pi_state_store: new_shared_pi_state_store(),
            pending_update: update::new_shared_pending_update(),
        };

        let response = list_tmux(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let response_json: Value = serde_json::from_slice(&body).unwrap();
        let registrations = response_json.as_array().unwrap();
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0]["session"], Value::from("issue-105"));
        assert_eq!(
            registrations[0]["registration_source"],
            Value::from("cli-watch")
        );
        assert_eq!(
            registrations[0]["registered_at"],
            Value::from("2026-04-02T00:00:00Z")
        );
        assert_eq!(registrations[0]["parent_process"]["pid"], Value::from(4242));
        assert_eq!(
            registrations[0]["parent_process"]["name"],
            Value::from("codex")
        );
    }

    #[tokio::test]
    async fn update_status_returns_no_pending_when_empty() {
        let (tx, _rx) = mpsc::channel(1);
        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: update::new_shared_pending_update(),
        };

        let response = update_status(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["pending"], Value::Bool(false));
        assert_eq!(json["current_version"], Value::String(VERSION.to_string()));
    }

    #[tokio::test]
    async fn update_status_returns_pending_when_set() {
        let (tx, _rx) = mpsc::channel(1);
        let pending = update::new_shared_pending_update();
        *pending.write().await = Some(update::PendingUpdate {
            current_version: "0.5.4".into(),
            latest_version: "0.6.0".into(),
            release_url: "https://github.com/Yeachan-Heo/clawhip/releases/tag/v0.6.0".into(),
            detected_at: "2026-04-07T00:00:00Z".into(),
        });

        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: pending,
        };

        let response = update_status(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["pending"], Value::Bool(true));
        assert_eq!(json["latest_version"], Value::from("0.6.0"));
        assert_eq!(json["current_version"], Value::from("0.5.4"));
    }

    #[tokio::test]
    async fn approve_returns_error_when_no_pending_update() {
        let (tx, _rx) = mpsc::channel(1);
        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: update::new_shared_pending_update(),
        };

        let response = approve_update(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], Value::Bool(false));
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("no pending update")
        );
    }

    #[tokio::test]
    async fn dismiss_clears_pending_update() {
        let (tx, _rx) = mpsc::channel(1);
        let pending = update::new_shared_pending_update();
        *pending.write().await = Some(update::PendingUpdate {
            current_version: "0.5.4".into(),
            latest_version: "0.6.0".into(),
            release_url: "https://example.com".into(),
            detected_at: "2026-04-07T00:00:00Z".into(),
        });

        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: pending.clone(),
        };

        let response = dismiss_update(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], Value::Bool(true));
        assert_eq!(json["dismissed_version"], Value::from("0.6.0"));
        assert!(pending.read().await.is_none());
    }

    #[tokio::test]
    async fn dismiss_returns_error_when_no_pending_update() {
        let (tx, _rx) = mpsc::channel(1);
        let state = AppState {
            config: Arc::new(AppConfig::default()),
            port: 25294,
            tx,
            tmux_registry: Arc::new(RwLock::new(HashMap::new())),
            pi_state_store: new_shared_pi_state_store(),
            pending_update: update::new_shared_pending_update(),
        };

        let response = dismiss_update(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], Value::Bool(false));
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
