# Pi × clawhip Integration Plan

This document is the execution-oriented integration spec for making Pi work well with clawhip while preserving a visible operator-first workflow.

## 1. Goal and scope

### Goal
Make Pi work with clawhip in a way that matches the current operator need: visible ongoing sessions first, richer structured integration later.

- Pi runs as the execution tool
- Pi remains directly visible in tmux during execution
- clawhip provides monitoring, mentions, formatting, and delivery
- structured Pi-native event integration remains a later expansion path

### Scope in

- Pi wrapper integration under `skills/pi/`
- Pi plugin scaffold under `plugins/pi/`
- tmux-first Layer A workflow documentation and refinement
- docs and live-verification coverage for Pi
- optional wrapper lifecycle emits as a secondary signal

### Scope out

- changing clawhip’s fundamental router architecture
- Pi-internal product redesign unrelated to event emission
- replacing tmux monitoring entirely
- adding a Pi-only custom event family unless required later

## 2. Current execution snapshot

clawhip already has the right architectural primitives for a first-phase Pi integration:

- tmux wrapper + watch flow for monitored tool execution
- wrapper patterns in `skills/omc/` and `skills/omx/`
- plugin scaffolds in `plugins/codex/` and `plugins/claude-code/`
- native canonical routing on `session.*` for later structured integration
- route-level control over channel, mentions, and formatting

Current gap:

- Pi does not yet have a first-class Layer A operator workflow in this repo
- Pi docs must clearly reflect tmux-first operation as the current primary path
- there is no Pi-specific live verification path yet

## 3. Proposed documentation architecture

### Canonical source of truth

- `docs/pi-integration-plan.md` = implementation plan and architecture contract for Pi
- `docs/pi-mono-architecture-notes.md` = what matters about Pi’s real architecture
- `docs/pi-layer-a-operator-guide.md` = tmux-first visible-session workflow
- `docs/native-event-contract.md` = canonical event normalization and routing contract for later structured integration
- `docs/live-verification.md` = canonical verification checklist
- `README.md` = entry-level operator overview and pointers

### Relationship to existing docs

- this document defines execution order and phase decisions
- `pi-layer-a-operator-guide.md` defines the current primary operator workflow
- `pi-mono-architecture-notes.md` explains why Pi should not be treated as just a renamed OMC/OMX target
- `native-event-contract.md` remains the source of truth for future structured Pi-native routing

## 4. Integration model

## 4.1 Preferred architecture

Current primary model:

```text
Pi TUI in tmux -> clawhip observability around the live session -> keyword/stale/state collection -> route/render/sinks
```

Optional later auxiliary model:

```text
Pi JSON/RPC/extension signals -> enrich clawhip state when TUI/tmux observation alone is insufficient
```

### Design principle

Pi compatibility should mean:

- Layer A keeps Pi directly observable in tmux
- clawhip owns monitoring, routing, and delivery around that live session
- richer native Pi event ingestion should complement the visible workflow later, not replace it

## 4.2 Compatibility levels

### Level 1 — tmux-only compatibility
Pi runs in tmux and clawhip watches output.

**Value**
- zero or minimal Pi-side work
- immediate operator usability
- best visibility for ongoing sessions

**Limitations**
- brittle keyword dependence
- poor structured state
- weak filtering and reporting fidelity

### Level 2 — wrapper-assisted Layer A compatibility
A Pi wrapper launches Pi in tmux and adds lightweight lifecycle emits on top of the visible session.

**Value**
- practical to ship quickly
- keeps operator visibility first
- gives optional session-level notifications alongside tmux alerts

**Limitations**
- still adapter-driven
- richer states depend on available Pi hooks/output signals
- tmux monitoring remains the main observability path

### Level 3 — native Pi event compatibility
Pi directly emits native structured events or native JSON payloads that clawhip normalizes.

**Value**
- best long-term architecture
- strongest route/filter stability
- least ambiguity in notification behavior

**Limitations**
- requires Pi-side hook/event support or a dedicated bridge

### Decision
Ship Layer A first using tmux-first visible sessions. Keep Level 2 wrapper emits as a secondary signal, and treat Level 3 Pi-native structured integration as the intended next major step after the operator workflow is solid.

## 5. Canonical event contract for Pi

Pi can eventually align with the existing clawhip canonical event family:

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

### Current rule

For the current Layer A phase:

- `tmux.*` is the primary operational routing surface
- wrapper `session.*` emits are optional secondary signals
- Pi-native structured routing is intentionally deferred to a later phase

## 6. Planned file changes

### Create

- `docs/pi-integration-plan.md`
- `docs/pi-mono-architecture-notes.md`
- `docs/pi-layer-a-operator-guide.md`
- `skills/pi/SKILL.md`
- `skills/pi/create.sh`
- `skills/pi/prompt.sh`
- `skills/pi/tail.sh`
- `plugins/pi/plugin.toml`
- `plugins/pi/bridge.sh`

### Update

- `README.md`
- `docs/live-verification.md`
- later: `docs/native-event-contract.md`
- optionally `SKILL.md` if Pi should appear in the operator runtime surface

### Leave alone initially

- routing core
- sink architecture
- memory subsystem
- Discord/Slack transport modules

## 7. Planned implementation phases

## Phase 1 — Repo-level Pi scaffolding

Create Pi surfaces modeled after the existing OMC/OMX structure where useful, but document Pi as an operator-first tmux workflow rather than a copy of OMC/OMX native behavior.

### Deliverables

- `skills/pi/` directory with usage guidance and wrapper scripts
- `plugins/pi/` directory with metadata + bridge stub
- README pointer to Pi docs

### Success criteria

- repo visibly supports Pi as an integration target
- humans and agents can discover the planned integration path quickly

## Phase 2 — Layer A tmux-first Pi integration

Implement a Pi wrapper that launches Pi in a monitored tmux session for direct operator visibility.

### Initial behavior

- keep Pi visible and attachable in tmux
- preserve tmux keyword/stale monitoring as the main alerting path
- emit session start and finish/failure as a secondary signal
- pass through channel and mention options cleanly

### Preferred interface shape

```bash
skills/pi/create.sh <session-name> <worktree-path> [prompt] [channel-id] [mention]
```

### Notes

- follow the same operator ergonomics as `skills/omc/create.sh` and `skills/omx/create.sh` where that helps
- keep env-based overrides for flags, keywords, stale timeout, and Pi environment
- optimize for attachability and observability first, not semantic perfection

## Phase 3 — Native Pi event bridge

Add a bridge path that allows Pi to emit native clawhip-compatible events without requiring tmux text scraping as the primary signal.

### Accepted bridge forms

#### Option A — Pi JSON mode bridge
Consume Pi JSON event output and map it into clawhip lifecycle/session events.

#### Option B — Pi RPC mode bridge
Use Pi RPC for deeper integration and process control while still preserving an operator-visible path where useful.

#### Option C — Pi extension/package bridge
Build a Pi-native extension or package that emits clawhip-compatible events directly.

### Decision

Do not make this the first shipped path. Build it after Layer A is reliable and well-documented.

## Phase 4 — Richer Pi semantic events

Once a structured bridge exists, support higher-value events:

- blocked / idle / question-needed
- PR created
- test started / finished / failed
- retry needed
- handoff needed

### Success criteria

- Pi event fidelity is high enough that Discord/Slack alerts can route by structured fields rather than terminal text alone
- tmux keyword monitoring becomes supplemental rather than primary for lifecycle awareness

## Phase 5 — Docs and verification closure

Update:

- `README.md`
- `docs/live-verification.md`
- later: `docs/native-event-contract.md`

Add Pi verification steps for:

- tmux launch / attach
- tmux keyword alerts
- tmux stale alerts
- wrapper start/finish/failure signals if enabled
- later structured bridge scenarios

## 8. Draft snippets

## 8.1 Example primary route

```toml
[[routes]]
event = "tmux.*"
sink = "discord"
channel = "YOUR_CHANNEL_ID"
mention = "<@YOUR_USER_ID>"
format = "alert"
allow_dynamic_tokens = false
```

## 8.2 Example secondary route

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

## 8.3 Example wrapper behavior

```text
1. start Pi in tmux via clawhip tmux new
2. keep the session visible and attachable
3. capture keyword and stale alerts via tmux monitoring
4. optionally emit session.started with tool=pi
5. emit session.finished or session.failed on exit
```

## 9. Risks and open decisions

### Risks

- tmux keyword dependence can hide true semantic state if used too heavily
- Pi output wording may change and affect alert quality
- wrapper emits may give a false sense of structured certainty if over-relied on too early

### Open decisions

- what keyword set works best for real Pi sessions in practice?
- should wrapper `session.*` emits remain enabled by default, or be optional later?
- when should a JSON/RPC bridge become worth the added complexity?
- should Pi route examples encourage only `tmux.*` first, or present both `tmux.*` and `session.*` equally?

### Recommended decisions

- keep tmux-first Layer A as the primary shipped workflow
- use wrapper `session.*` emits only as a secondary signal at first
- keep plugin bridge minimal at first
- tune keyword defaults based on real Pi session transcripts

## 10. Verification checklist

### Documentation verification

- [ ] README links to Pi plan
- [ ] Pi architecture notes explain why Pi is not just OMC/OMX renamed
- [ ] Layer A guide clearly documents attach/tail/watch workflow
- [ ] live verification doc includes Pi scenarios

### Implementation verification

- [ ] Pi wrapper can launch a monitored visible session
- [ ] `tmux attach -t <session>` works as the main live-view path
- [ ] keyword alerts reach the target channel
- [ ] stale alerts reach the target channel
- [ ] wrapper start/finish/failure signals work if enabled
- [ ] no existing OMC/OMX flows regress

## 11. Recommended next execution stage

After this planning doc lands, the next implementation sequence should be:

1. scaffold `skills/pi/`
2. scaffold `plugins/pi/`
3. implement Layer A tmux-first Pi workflow refinements
4. update live verification docs for Pi
5. later, design the JSON/RPC/extension bridge
6. add or update tests only where new normalization or routing behavior is introduced
