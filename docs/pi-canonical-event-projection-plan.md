# Pi Canonical Event Projection Plan

## Docs Principle Result

### 1. Goal and scope

This document defines the first implementation slice for projecting **Pi tmux-first observations** into the canonical clawhip `session.*` event family.

The immediate goal is **not** to mirror full Pi transcripts into Discord/Slack.
The goal is to make Pi operationally comparable to OMC/OMX by turning stable, operator-relevant state changes into canonical events.

This first slice focuses on two high-value projections:

- `session.blocked`
- `session.pr-created`

These are intentionally chosen because they are:

- useful in real operator workflows,
- compatible with the existing clawhip router/rendering contract,
- and achievable from current tmux/state evidence without waiting for Pi-native JSON/RPC integration.

### 2. Current execution snapshot

Current Pi integration in clawhip already supports:

- visible Pi sessions in tmux,
- prompt injection,
- tail/attach operator workflow,
- stale/keyword monitoring,
- wrapper lifecycle emits,
- and a first in-memory Pi session state model.

What is still missing relative to OMC/OMX-style canonical routing is a projection layer that promotes Pi state changes into `session.*` events.

At the time of this document:

- `session.started` is available through wrapper lifecycle emits,
- `session.finished` / `session.failed` may be available through wrapper lifecycle emits,
- `tmux.keyword` and `tmux.stale` exist as fallback/operator signals,
- but Pi-specific semantic events are not yet projected into the canonical contract.

### 3. Proposed documentation architecture

This plan sits between the existing Pi state docs and eventual broader parity docs.

Recommended source-of-truth roles:

- `docs/pi-tui-observability-contract.md`
  - runtime observability principles
- `docs/pi-session-state-model.md`
  - internal Pi state object and transitions
- `docs/pi-session-state-implementation-plan.md`
  - state implementation in the codebase
- `docs/pi-canonical-event-projection-plan.md`
  - how Pi state becomes canonical clawhip `session.*` events
- `docs/native-event-contract.md`
  - the canonical cross-tool routing contract that Pi should project into

### 4. Planned file changes

#### Create

- `docs/pi-canonical-event-projection-plan.md`

#### Update

- `src/source/tmux.rs`
  - add a lightweight Pi projection layer on top of existing tmux/state observation
  - dedupe repeated semantic emits per session
  - emit canonical `session.blocked` when Pi transitions into blocked/waiting
  - emit canonical `session.pr-created` when PR creation evidence is strong enough

#### Leave alone in this slice

- `src/router.rs`
- `src/render/*`
- `src/event/compat.rs`
- `src/daemon.rs`

Those already understand the canonical `session.*` family well enough for this first projection slice.

### 5. Draft snippets

#### 5.1 Projection model

```text
raw tmux observation
-> Pi in-memory state update
-> dedup/projection memo
-> canonical session.* event emit
```

Important rule:

```text
raw pane text must not route directly to sinks as canonical semantic truth
```

Instead:

```text
pane evidence -> Pi state / semantic inference -> canonical event projection
```

#### 5.2 First-slice event definitions

##### `session.blocked`

Emit when all of the following are true:

- session is identified as `tool = pi`
- session remains alive in tmux
- Pi state activity becomes `blocked-or-waiting`
- the transition is new for that session (deduped)

Suggested payload shape:

```json
{
  "tool": "pi",
  "session_name": "issue-123",
  "session_id": "issue-123",
  "repo_name": "clawhip",
  "repo_path": "/path/to/repo",
  "status": "blocked",
  "summary": "Pi appears to be waiting for operator input",
  "mention": "<@user>",
  "contract_event": "session.blocked"
}
```

##### `session.pr-created`

Emit when all of the following are true:

- session is identified as `tool = pi`
- pane output contains strong PR creation evidence
- the evidence is new for that session (deduped)

Accepted strong evidence in this first slice:

- a line containing `PR created`
- a GitHub pull request URL
- a line clearly stating a pull request was created

Suggested payload shape:

```json
{
  "tool": "pi",
  "session_name": "issue-123",
  "session_id": "issue-123",
  "repo_name": "clawhip",
  "repo_path": "/path/to/repo",
  "pr_number": 71,
  "pr_url": "https://github.com/org/repo/pull/71",
  "status": "pr-created",
  "summary": "Pi reported PR creation",
  "mention": "<@user>",
  "contract_event": "session.pr-created"
}
```

#### 5.3 Dedup policy

Projection must be deduped per session.

Recommended first implementation:

- track whether `session.blocked` is currently active for the session
- track the last emitted PR evidence key for the session

This avoids repeated spam on every tmux poll tick.

#### 5.4 Non-goals for this slice

Do **not** implement these yet in the same pass:

- transcript relay to Discord/Slack
- `session.retry-needed`
- `session.test-started` / `session.test-finished` / `session.test-failed`
- `session.handoff-needed`
- tool event projection
- cycle event projection

These remain future phases.

### 6. Risks and open decisions

#### Risks

- blocked/waiting inference is still heuristic in tmux-first mode
- PR creation detection may overfit to visible wording if the first heuristics are too broad
- repeated pane churn can still cause event spam if dedup state is too weak

#### Open decisions

- whether `session.retry-needed` should be inferred from blocked state or only from stronger textual evidence
- whether future projection should live in a dedicated module (for example `src/pi_projection.rs`) or remain inside `src/source/tmux.rs`
- whether PR creation should require a URL in the future for higher confidence

### 7. Verification checklist

- [ ] Pi wrapper sessions still appear in `/api/pi/state`
- [ ] canonical `session.blocked` emits only once per blocked transition
- [ ] canonical `session.pr-created` emits only once per new PR evidence
- [ ] existing `tmux.keyword` behavior does not regress
- [ ] existing wrapper lifecycle emits do not regress
- [ ] Discord/Slack rendering works through existing `session.*` formatting
