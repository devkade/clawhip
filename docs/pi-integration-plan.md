# Pi × clawhip Integration Plan

This document is the execution-oriented integration spec for making Pi a first-class clawhip-compatible session producer.

## 1. Goal and scope

### Goal
Make Pi work with clawhip at the same architectural level as OMC/OMX:

- Pi runs as the execution tool
- Pi emits structured operational events
- clawhip owns routing, mention policy, formatting, and delivery
- tmux monitoring remains available as a fallback and operator convenience layer

### Scope in

- Pi wrapper integration under `skills/pi/`
- Pi plugin scaffold under `plugins/pi/`
- native event contract alignment on `session.*`
- docs and live-verification coverage for Pi
- route examples for `tool = "pi"`

### Scope out

- changing clawhip’s fundamental router architecture
- Pi-internal product redesign unrelated to event emission
- replacing tmux monitoring entirely
- adding a Pi-only custom event family unless required later

## 2. Current execution snapshot

clawhip already has the right architectural primitives for Pi compatibility:

- native canonical routing on `session.*`
- legacy compatibility on `agent.*`
- wrapper patterns in `skills/omc/` and `skills/omx/`
- plugin scaffolds in `plugins/codex/` and `plugins/claude-code/`
- tmux wrapper + watch flow for monitored tool execution
- native normalization support for upstream payloads carrying:
  - `signal.routeKey`
  - `context.normalized_event`

Current gap:

- Pi does not yet have a first-class wrapper/plugin/docs surface in this repo
- Pi is not yet documented as a native `session.*` producer
- there is no Pi-specific live verification path

## 3. Proposed documentation architecture

### Canonical source of truth

- `docs/pi-integration-plan.md` = implementation plan and architecture contract for Pi
- `docs/native-event-contract.md` = canonical event normalization and routing contract
- `docs/live-verification.md` = canonical verification checklist
- `README.md` = entry-level operator overview and pointers

### Relationship to existing docs

- do not duplicate the full event contract here
- this document should describe Pi-specific integration decisions and delivery plan
- event-kind semantics remain canonical in `docs/native-event-contract.md`
- operator-facing quick references remain in `README.md`

## 4. Integration model

## 4.1 Preferred architecture

Preferred model:

```text
Pi -> native session/event emit -> clawhip normalization/routing -> Discord/Slack
```

Fallback/operator model:

```text
Pi in tmux -> clawhip tmux watch/new -> keyword + stale alerts
```

### Design principle

Pi compatibility should mean:

- Pi becomes a producer of clawhip’s existing session contract
- clawhip remains the single routing and formatting authority
- tmux monitoring complements native events rather than replacing them

## 4.2 Compatibility levels

### Level 1 — tmux-only compatibility
Pi runs in tmux and clawhip watches output.

**Value**
- zero or minimal Pi-side work
- immediate operator usability

**Limitations**
- brittle keyword dependence
- poor structured state
- weak filtering and reporting fidelity

### Level 2 — wrapper-based session compatibility
A Pi wrapper launches Pi and emits lifecycle/session events into clawhip.

**Value**
- practical to ship quickly
- aligns with current OMC/OMX wrapper model
- enables first-class routing by structured metadata

**Limitations**
- still adapter-driven
- richer states depend on available Pi hooks/output signals

### Level 3 — native Pi event compatibility
Pi directly emits native structured events or native JSON payloads that clawhip normalizes.

**Value**
- best long-term architecture
- strongest route/filter stability
- least ambiguity in notification behavior

**Limitations**
- requires Pi-side hook/event support

### Decision
Build Level 2 first, but target Level 3 as the intended end state.

## 5. Canonical event contract for Pi

Pi should align with the existing clawhip canonical event family:

- `session.started`
- `session.blocked`
- `session.finished`
- `session.failed`
- `session.retry-needed`
- `session.pr-created`
- `session.test-started`
- `session.test-finished`
- `session.test-failed`
- `session.handoff-needed`

### Required normalized metadata

When Pi or the Pi bridge can provide them, normalize these fields:

- `tool = "pi"`
- `tool_name = "pi"`
- `session_name`
- `session_id`
- `repo_name`
- `repo_path`
- `worktree_path`
- `branch`
- `issue_number`
- `pr_number`
- `pr_url`
- `command`
- `summary`
- `status`
- `error_message`
- `event_timestamp`
- `contract_event`

### Contract rule

New Pi routes should target `session.*`, not `agent.*`.

Legacy wrapper compatibility may still emit `agent.*` during transition if doing so reduces implementation friction, but Pi docs and examples should prefer `session.*`.

## 6. Planned file changes

### Create

- `docs/pi-integration-plan.md`
- `skills/pi/SKILL.md`
- `skills/pi/create.sh`
- `skills/pi/prompt.sh`
- `skills/pi/tail.sh`
- `plugins/pi/plugin.toml`
- `plugins/pi/bridge.sh`

### Update

- `README.md`
- `docs/native-event-contract.md`
- `docs/live-verification.md`
- optionally `SKILL.md` if Pi should appear in the operator runtime surface

### Leave alone initially

- routing core
- sink architecture
- memory subsystem
- Discord/Slack transport modules

## 7. Planned implementation phases

## Phase 1 — Repo-level Pi scaffolding

Create Pi surfaces modeled after the existing OMC/OMX structure.

### Deliverables

- `skills/pi/` directory with usage guidance and wrapper scripts
- `plugins/pi/` directory with metadata + bridge stub
- README pointer to Pi integration plan

### Success criteria

- repo visibly supports Pi as an integration target
- humans and agents can discover the planned integration path quickly

## Phase 2 — Wrapper-based Pi lifecycle integration

Implement a Pi wrapper that can launch Pi in a monitored tmux session and emit lifecycle events.

### Initial behavior

- emit session start
- emit session finish or failure on exit
- preserve tmux keyword/stale monitoring
- pass through channel and mention options cleanly

### Preferred interface shape

```bash
skills/pi/create.sh <session-name> <worktree-path> [prompt] [channel-id] [mention]
```

### Notes

- follow the same operator ergonomics as `skills/omc/create.sh` and `skills/omx/create.sh`
- keep env-based overrides for flags, keywords, stale timeout, and Pi environment

## Phase 3 — Native Pi event bridge

Add a bridge path that allows Pi to emit native clawhip-compatible events without requiring tmux text scraping as the primary signal.

### Accepted bridge forms

#### Option A — CLI bridge
Pi or the wrapper invokes clawhip commands directly.

Example shape:

```bash
clawhip emit session.started ...
clawhip emit session.finished ...
```

#### Option B — native JSON payload bridge
Pi emits machine-readable payloads containing a route key / normalized event surface that clawhip maps into `session.*`.

Example shape:

- `context.normalized_event = "started"`
- `context.normalized_event = "finished"`
- `context.normalized_event = "pr-created"`

### Decision

Implement Option A first if it is faster.
Add Option B when Pi has stable hook output or native event emission support.

## Phase 4 — Richer Pi semantic events

Once baseline lifecycle compatibility exists, support higher-value events:

- blocked / idle / question-needed
- PR created
- test started / finished / failed
- retry needed
- handoff needed

### Success criteria

- Pi event fidelity is high enough that Discord/Slack alerts can route by structured fields rather than text keywords
- tmux keyword monitoring is no longer the primary status mechanism

## Phase 5 — Docs and verification closure

Update:

- `README.md`
- `docs/native-event-contract.md`
- `docs/live-verification.md`

Add Pi verification steps for:

- started
- blocked
- finished
- failed
- PR created
- test state events where available
- tmux stale fallback
- tmux keyword fallback

## 8. Draft snippets

## 8.1 Example route

```toml
[[routes]]
event = "session.*"
filter = { tool = "pi" }
sink = "discord"
channel = "YOUR_CHANNEL_ID"
mention = "<@YOUR_USER_ID>"
format = "compact"
allow_dynamic_tokens = false
```

## 8.2 Example wrapper behavior

```text
1. start Pi in tmux via clawhip tmux new
2. emit session.started with tool=pi
3. capture exit code and elapsed time
4. emit session.finished on success
5. emit session.failed on non-zero exit
6. continue tmux keyword/stale monitoring as secondary signal
```

## 8.3 Example long-term native payload shape

```json
{
  "context": {
    "normalized_event": "pr-created"
  },
  "tool": "pi",
  "session_name": "issue-123",
  "repo_name": "my-repo",
  "branch": "issue-123",
  "pr_number": 42,
  "pr_url": "https://github.com/org/repo/pull/42",
  "summary": "Pi created a pull request"
}
```

## 9. Risks and open decisions

### Risks

- Pi may not yet expose enough structured hooks for rich native events
- copying the OMC/OMX wrapper pattern too literally may lock Pi into legacy `agent.*` behavior
- tmux keyword dependence can hide true semantic state if used too heavily

### Open decisions

- should Pi wrapper emits be `session.*` from day one, or use temporary `agent.*` compatibility emits first?
- should the plugin bridge remain a stub initially, or should it immediately support event emission helpers?
- how much Pi-specific metadata should be required vs optional in early releases?
- should Pi route examples filter on `tool = "pi"` only, or also encourage `repo_name`, `session_name`, and `branch` filters by default?

### Recommended decisions

- prefer `session.*` from day one if the CLI surface supports it cleanly
- keep plugin bridge minimal at first
- make metadata additive, not mandatory, except for `tool`, event kind, and session identity
- document `tool = "pi"` as the minimum route filter and structured repo/session filters as best practice

## 10. Verification checklist

### Documentation verification

- [ ] README links to Pi plan
- [ ] Pi plan does not duplicate the canonical event contract unnecessarily
- [ ] native contract doc explicitly mentions Pi as a compatible producer target
- [ ] live verification doc includes Pi scenarios

### Implementation verification

- [ ] Pi wrapper can launch a monitored session
- [ ] start event routes correctly
- [ ] finish event routes correctly
- [ ] failure event routes correctly
- [ ] Pi route filtering on `tool = "pi"` works
- [ ] tmux fallback alerts still work
- [ ] no existing OMC/OMX flows regress

## 11. Recommended next execution stage

After this planning doc lands, the next implementation sequence should be:

1. scaffold `skills/pi/`
2. scaffold `plugins/pi/`
3. implement wrapper-based Pi lifecycle emits
4. update native event contract docs
5. extend live verification for Pi
6. add or update tests only where new normalization or routing behavior is introduced
