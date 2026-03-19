# Pi Session State Implementation Plan

## Purpose

This document translates the Pi TUI observability contract and Pi session state model into a concrete implementation plan for the current clawhip codebase.

It is intentionally scoped to **v1**:

- TUI/tmux-first
- state model before advanced semantics
- minimal file changes with clear extension points
- no requirement for JSON/RPC as the primary runtime path

---

## 1. Current codebase fit

Based on the current clawhip source tree, the most relevant existing implementation surfaces are:

- `src/source/tmux.rs`
  - existing tmux session polling, pane snapshotting, keyword detection, stale detection
- `src/tmux_wrapper.rs`
  - session launch/watch registration path for tmux-monitored runs
- `src/events.rs`
  - canonical incoming event representation and normalization
- `src/event/compat.rs`
  - typed compatibility layer for normalized events
- `src/router.rs`, `src/render/*`, `src/sink/*`
  - later downstream consumers once state is worth routing/rendering

### Key conclusion

The best first implementation path is:

- add **Pi session state tracking next to the tmux source layer**
- avoid pushing this directly into sinks/renderers too early
- treat routing/rendering as later consumers of a better state model

---

## 2. v1 implementation target

The first implementation should deliver:

### Required
- a concrete in-memory state model for TUI-observed Pi sessions
- session existence tracking
- attachability tracking
- pane-change timestamp tracking
- idle/stale derivation
- finish/fail enrichment when wrapper emits are present

### Explicitly not required in v1
- cycle detection
- tool detection
- blocked/question-needed heuristics beyond placeholders
- JSON/RPC integration
- sink-specific rendering for new Pi-native state

---

## 3. Proposed file changes

## 3.1 New files

### `src/pi_state.rs`

Create a dedicated module for Pi session state definitions and transitions.

#### Responsibilities
- define the `PiSessionState` struct
- define lifecycle/activity/confidence enums
- provide transition/update helper methods
- provide state merge/update functions from tmux observation and wrapper signals

#### Suggested contents
- `PiLifecycle`
- `PiActivity`
- `PiConfidence`
- `PiEvidence`
- `PiSessionState`
- `PiStateUpdate`
- helper functions:
  - `mark_created`
  - `mark_running`
  - `mark_finished`
  - `mark_failed`
  - `mark_aborted`
  - `update_activity_from_pane_change`
  - `update_stale`

### `src/pi_state_store.rs`

Create a small runtime store for Pi session states.

#### Responsibilities
- own an in-memory map keyed by session name/id
- support upsert/get/remove operations
- update timestamps consistently
- optionally support debug dump/export later

#### Suggested shape

```rust
pub type SharedPiStateStore = Arc<RwLock<HashMap<String, PiSessionState>>>;
```

This should remain runtime-oriented, not persistence-heavy.

---

## 3.2 Existing files to update

### `src/main.rs`

#### Changes
- add `mod pi_state;`
- add `mod pi_state_store;`
- initialize the shared Pi state store near other runtime/shared state
- pass the state store into the tmux source / wrapper monitor path as needed

#### Goal
Make Pi state available to the parts of the runtime already observing tmux sessions.

### `src/source/tmux.rs`

This is the most important implementation surface for v1.

#### Changes
Add Pi-aware state updates on top of existing tmux polling.

#### Specific work
1. extend `RegisteredTmuxSession` with optional tool/session metadata if needed
   - at minimum, make it possible to know a session belongs to Pi
2. add or reuse a session filter for `tool = pi`
3. when a registered Pi session is observed:
   - upsert a Pi session state object
   - mark `created` / `running`
   - track `attachable=true` when session exists
   - update `lastPaneChangeAt` when pane content hash changes
   - derive `activity=active` on pane changes
   - derive `activity=idle` after a shorter quiet threshold
   - derive `stale=true` using existing stale logic
4. when a session disappears:
   - transition lifecycle using best available evidence
   - prefer wrapper exit info if present
   - otherwise fall back to approximate finished/failed handling

#### Important rule
Do **not** overload the existing tmux keyword event path to become the primary state model.
Keyword events should stay events; state should be maintained separately.

### `src/tmux_wrapper.rs`

#### Changes
- extend registration to optionally include tool/session metadata for Pi-tracked sessions
- ensure Pi-launched sessions are identifiable in the monitor path
- optionally write a small registration flag or payload indicating `tool=pi`

#### Goal
Make sure sessions created through `skills/pi/create.sh` are not just generic tmux sessions from the state model’s perspective.

### `src/events.rs`

#### Changes
Possibly add a small custom event family later for internal debugging/export only, but **not required in v1**.

For v1, the most important role of this file is:
- continue to normalize wrapper lifecycle emits that already exist
- provide a future place to expose state snapshots if that becomes useful

#### Recommendation
Keep changes here minimal in v1.

### `src/event/compat.rs`

#### Changes
None required for the first state model unless new events are introduced.

#### Recommendation
Leave this unchanged in v1 unless you choose to expose state transitions as real events.

---

## 4. Registration and identification plan

The state engine needs to know which tmux sessions should be treated as Pi sessions.

### Recommended first method
Use wrapper registration metadata.

When `skills/pi/create.sh` launches via `clawhip tmux new`, ensure the registered session can be identified as:

- `tool = pi`
- `session_name = <name>`
- `repo_path = <path>`
- optionally `project = <detected project>`

### Why this matters
Without identification, the state engine would have to guess from pane text or session naming alone.
That is too brittle.

---

## 5. v1 state update algorithm

## 5.1 On session registration / initial observation

When a Pi session is first seen and tmux existence is confirmed:

- create `PiSessionState` if absent
- set:
  - `lifecycle = created`
  - `attachable = true`
  - `stale = false`
  - `sources.tmuxExists = true`
- after first successful pane observation or grace period:
  - transition to `lifecycle = running`

## 5.2 On pane content change

When the pane hash changes:

- update `lastPaneChangeAt`
- set `activity = active`
- set `confidence.activity = medium` or `low` depending on signal quality
- set `sources.paneObserved = true`
- clear stale if desired and if recovery semantics are allowed

## 5.3 On quiet interval

If pane content does not change for an **idle threshold** shorter than stale threshold:

- set `activity = idle`
- do not change lifecycle yet

Suggested initial value:
- idle threshold = 2 to 5 minutes

## 5.4 On stale interval

When existing stale logic fires:

- set `stale = true`
- keep lifecycle as `running` unless stronger evidence says otherwise
- optionally set `activity = blocked-or-waiting` only if heuristic evidence exists

## 5.5 On wrapper lifecycle emit

If wrapper emits are available and reach the daemon reliably:

- `session.started` -> reinforce `running`
- `session.finished` -> set `finished`, `exitCode = 0` if known
- `session.failed` -> set `failed`, store failure reason / non-zero exit if known

### Important rule
Wrapper emits should **enrich** state, not be required for state existence.

## 5.6 On tmux disappearance

When the session no longer exists:

- if recent wrapper clean exit exists -> `finished`
- else if recent wrapper failure exists -> `failed`
- else classify conservatively:
  - `finished` with low confidence, or
  - `aborted` if explicit operator kill is known

Recommendation for v1:
- use `finished` with low confidence unless explicit failure/abort evidence exists

---

## 6. Suggested data structures

## 6.1 Enums

```rust
pub enum PiLifecycle {
    Created,
    Running,
    Finished,
    Failed,
    Aborted,
    Unknown,
}

pub enum PiActivity {
    Active,
    Idle,
    BlockedOrWaiting,
    Unknown,
}

pub enum PiConfidence {
    High,
    Medium,
    Low,
}
```

## 6.2 State struct

```rust
pub struct PiSessionState {
    pub tool: String,
    pub session_name: String,
    pub session_id: String,
    pub project: String,
    pub repo_path: String,
    pub tmux_session: String,

    pub lifecycle: PiLifecycle,
    pub activity: PiActivity,
    pub stale: bool,
    pub attachable: bool,

    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub last_pane_change_at_unix: Option<u64>,
    pub last_prompt_inject_at_unix: Option<u64>,
    pub last_observed_text: Option<String>,

    pub exit_code: Option<i32>,
    pub failure_reason: Option<String>,

    pub lifecycle_confidence: PiConfidence,
    pub activity_confidence: PiConfidence,

    pub source_tmux_exists: bool,
    pub source_pane_observed: bool,
    pub source_wrapper_emit: bool,
    pub source_auxiliary: bool,
}
```

### Note
This can be tightened/reduced if the Rust implementation prefers nested structs, but the semantic content should remain.

---

## 7. Polling and cadence

## Recommended cadence

Reuse the existing tmux poll cadence where possible.

### For v1
- use the existing tmux polling loop
- compute pane-change state from existing snapshot hashes
- compute idle from `last_change`
- compute stale from existing stale logic

### Benefit
This avoids introducing a second competing poll loop too early.

---

## 8. Persistence recommendation

## v1 recommendation
Start with:
- in-memory shared state store only

## v1.1 recommendation
Add optional debug export:
- JSONL state transition log, or
- dump command for current Pi session states

### Why
The state model should be usable before persistence is solved.

---

## 9. Testing plan

## Unit tests to add

### `src/pi_state.rs`
- lifecycle transition tests
- activity derivation tests
- stale flag behavior tests
- confidence merge/update tests

### `src/pi_state_store.rs`
- upsert/get/remove tests
- update timestamp behavior tests

### `src/source/tmux.rs`
Add targeted tests for Pi state updates:
- first observation creates state
- pane change marks active
- quiet interval marks idle
- stale logic sets stale flag
- disappearance after wrapper fail marks failed
- disappearance without explicit failure stays conservative

## Integration tests later
- launch a Pi session through `skills/pi/create.sh`
- verify store state changes over time
- verify attachability and stale derivation

---

## 10. Non-goals for this implementation slice

Do not include these in the first coding pass:

- tool boundary inference
- cycle boundary inference
- blocked/question-needed phrase heuristics
- JSON/RPC ingestion
- route/sink exposure of the new state model
- dashboard/UI work

---

## 11. Recommended execution order

### Phase 1
Create:
- `src/pi_state.rs`
- `src/pi_state_store.rs`

### Phase 2
Wire store initialization in:
- `src/main.rs`

### Phase 3
Integrate Pi state updates into:
- `src/source/tmux.rs`

### Phase 4
Add registration/tool metadata support in:
- `src/tmux_wrapper.rs`
- and, if needed, wrapper registration payload paths

### Phase 5
Add tests for state transitions and tmux-derived state updates

---

## 12. Success criteria for v1

v1 is successful when:

- Pi sessions launched in tmux have an internal state object
- state updates happen from real tmux observation
- idle/stale are derived without JSON/RPC
- finished/failed can be enriched by wrapper emits when present
- the model is useful for later routing/rendering, even if no sink uses it yet

---

## 13. Recommended immediate next code step

The first concrete coding step should be:

1. create `src/pi_state.rs`
2. define the enums + `PiSessionState`
3. create `src/pi_state_store.rs`
4. wire a minimal store into `src/main.rs`
5. teach `src/source/tmux.rs` to upsert basic Pi state for identified Pi sessions

That is the smallest implementation slice that turns the docs into a working runtime model.
