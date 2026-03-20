use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PiLifecycle {
    Created,
    #[default]
    Running,
    Finished,
    Failed,
    Aborted,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PiActivity {
    Active,
    Idle,
    BlockedOrWaiting,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PiConfidence {
    High,
    Medium,
    #[default]
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PiStateConfidence {
    pub lifecycle: PiConfidence,
    pub activity: PiConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PiStateSources {
    pub tmux_exists: bool,
    pub pane_observed: bool,
    pub wrapper_emit: bool,
    pub auxiliary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PiSessionState {
    pub tool: String,
    pub session_name: String,
    pub session_id: String,
    pub project: String,
    pub repo_path: String,
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    pub lifecycle: PiLifecycle,
    pub activity: PiActivity,
    pub stale: bool,
    pub attachable: bool,
    pub cycle_count: u64,
    pub cycle_active: bool,

    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pane_change_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_cycle_started_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_cycle_ended_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_prompt_inject_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_observed_text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,

    pub confidence: PiStateConfidence,
    pub sources: PiStateSources,
}

impl PiSessionState {
    pub fn new(
        session_name: String,
        project: String,
        repo_path: String,
        tmux_session: String,
        branch: Option<String>,
    ) -> Self {
        let now = unix_now();
        Self {
            tool: "pi".to_string(),
            session_id: session_name.clone(),
            session_name,
            project,
            repo_path,
            tmux_session,
            branch,
            lifecycle: PiLifecycle::Created,
            activity: PiActivity::Unknown,
            stale: false,
            attachable: true,
            cycle_count: 0,
            cycle_active: false,
            created_at_unix: now,
            updated_at_unix: now,
            last_pane_change_at_unix: None,
            current_cycle_started_at_unix: None,
            last_cycle_ended_at_unix: None,
            last_prompt_inject_at_unix: None,
            last_observed_text: None,
            exit_code: None,
            failure_reason: None,
            confidence: PiStateConfidence {
                lifecycle: PiConfidence::Medium,
                activity: PiConfidence::Low,
            },
            sources: PiStateSources {
                tmux_exists: true,
                pane_observed: false,
                wrapper_emit: false,
                auxiliary: false,
            },
        }
    }

    pub fn touch(&mut self, now: u64) {
        self.updated_at_unix = now;
    }

    pub fn mark_running(&mut self, now: u64) {
        self.lifecycle = PiLifecycle::Running;
        self.attachable = true;
        self.sources.tmux_exists = true;
        self.confidence.lifecycle = PiConfidence::Medium;
        self.touch(now);
    }

    pub fn mark_pane_change(&mut self, now: u64, text: String) {
        self.start_cycle_if_needed(now);
        self.last_pane_change_at_unix = Some(now);
        self.last_observed_text = Some(text);
        self.activity = PiActivity::Active;
        self.attachable = true;
        self.stale = false;
        self.sources.tmux_exists = true;
        self.sources.pane_observed = true;
        self.confidence.activity = PiConfidence::Medium;
        if matches!(self.lifecycle, PiLifecycle::Created | PiLifecycle::Unknown) {
            self.lifecycle = PiLifecycle::Running;
            self.confidence.lifecycle = PiConfidence::Medium;
        }
        self.touch(now);
    }

    pub fn mark_idle(&mut self, now: u64) {
        self.end_cycle(now);
        self.activity = PiActivity::Idle;
        self.confidence.activity = PiConfidence::Low;
        self.touch(now);
    }

    pub fn mark_blocked_or_waiting(&mut self, now: u64) {
        self.end_cycle(now);
        self.activity = PiActivity::BlockedOrWaiting;
        self.confidence.activity = PiConfidence::Low;
        self.touch(now);
    }

    pub fn mark_stale(&mut self, now: u64) {
        self.end_cycle(now);
        self.stale = true;
        self.touch(now);
    }

    pub fn mark_finished(&mut self, now: u64, exit_code: Option<i32>) {
        self.end_cycle(now);
        self.lifecycle = PiLifecycle::Finished;
        self.activity = PiActivity::Unknown;
        self.attachable = false;
        self.stale = false;
        self.exit_code = exit_code.or(Some(0));
        self.confidence.lifecycle = PiConfidence::Low;
        self.touch(now);
    }

    pub fn mark_failed(&mut self, now: u64, reason: Option<String>, exit_code: Option<i32>) {
        self.end_cycle(now);
        self.lifecycle = PiLifecycle::Failed;
        self.activity = PiActivity::Unknown;
        self.attachable = false;
        self.stale = false;
        self.exit_code = exit_code;
        self.failure_reason = reason;
        self.confidence.lifecycle = PiConfidence::Low;
        self.touch(now);
    }

    pub fn mark_aborted(&mut self, now: u64) {
        self.end_cycle(now);
        self.lifecycle = PiLifecycle::Aborted;
        self.activity = PiActivity::Unknown;
        self.attachable = false;
        self.stale = false;
        self.confidence.lifecycle = PiConfidence::Low;
        self.touch(now);
    }

    fn start_cycle_if_needed(&mut self, now: u64) {
        if !self.cycle_active {
            self.cycle_active = true;
            self.cycle_count += 1;
            self.current_cycle_started_at_unix = Some(now);
        }
    }

    fn end_cycle(&mut self, now: u64) {
        if self.cycle_active {
            self.cycle_active = false;
            self.last_cycle_ended_at_unix = Some(now);
            self.current_cycle_started_at_unix = None;
        }
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_change_moves_session_to_running_active() {
        let mut state = PiSessionState::new(
            "issue-1".into(),
            "repo".into(),
            "/repo".into(),
            "issue-1".into(),
            None,
        );
        state.mark_pane_change(200, "hello".into());
        assert_eq!(state.lifecycle, PiLifecycle::Running);
        assert_eq!(state.activity, PiActivity::Active);
        assert_eq!(state.last_pane_change_at_unix, Some(200));
        assert_eq!(state.last_observed_text.as_deref(), Some("hello"));
    }

    #[test]
    fn stale_and_idle_updates_do_not_finish_session() {
        let mut state = PiSessionState::new(
            "issue-1".into(),
            "repo".into(),
            "/repo".into(),
            "issue-1".into(),
            None,
        );
        state.mark_running(100);
        state.mark_idle(120);
        state.mark_stale(180);
        assert_eq!(state.lifecycle, PiLifecycle::Running);
        assert_eq!(state.activity, PiActivity::Idle);
        assert!(state.stale);
    }

    #[test]
    fn blocked_waiting_state_is_distinct_from_idle() {
        let mut state = PiSessionState::new(
            "issue-1".into(),
            "repo".into(),
            "/repo".into(),
            "issue-1".into(),
            None,
        );
        state.mark_running(100);
        state.mark_blocked_or_waiting(140);
        assert_eq!(state.lifecycle, PiLifecycle::Running);
        assert_eq!(state.activity, PiActivity::BlockedOrWaiting);
        assert_eq!(state.confidence.activity, PiConfidence::Low);
    }

    #[test]
    fn pane_changes_start_cycle_and_idle_ends_it() {
        let mut state = PiSessionState::new(
            "issue-1".into(),
            "repo".into(),
            "/repo".into(),
            "issue-1".into(),
            None,
        );
        state.mark_running(100);
        state.mark_pane_change(120, "working".into());
        assert!(state.cycle_active);
        assert_eq!(state.cycle_count, 1);
        assert_eq!(state.current_cycle_started_at_unix, Some(120));

        state.mark_idle(160);
        assert!(!state.cycle_active);
        assert_eq!(state.last_cycle_ended_at_unix, Some(160));
        assert_eq!(state.current_cycle_started_at_unix, None);
    }

    #[test]
    fn new_activity_after_end_starts_new_cycle() {
        let mut state = PiSessionState::new(
            "issue-1".into(),
            "repo".into(),
            "/repo".into(),
            "issue-1".into(),
            None,
        );
        state.mark_running(100);
        state.mark_pane_change(120, "first burst".into());
        state.mark_idle(150);
        state.mark_pane_change(180, "second burst".into());
        assert!(state.cycle_active);
        assert_eq!(state.cycle_count, 2);
        assert_eq!(state.current_cycle_started_at_unix, Some(180));
    }
}
