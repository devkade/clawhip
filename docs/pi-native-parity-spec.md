# Pi Native Integration Parity Spec

## Docs Principle Result

### 1. Goal and scope

This document defines the target specification for **Pi native integration** in clawhip and compares it against the **currently implemented state**.

The target is **not**:

- a Pi-flavored copy of OMC,
- a tmux-only wrapper forever,
- or a notification-only integration.

The target **is**:

- Pi remains the execution backend,
- Pi native surfaces (`json`, `rpc`, extension/package hooks) become the semantic integration source,
- tmux remains the visible operator surface,
- and the total capability level should reach **OMC-class operational completeness**.

In short:

```text
Pi native integration
+ tmux visual observability
+ OMC-class lifecycle / operator / routing completeness
```

### Scope in

- target functional parity areas relative to OMC-class usage
- Pi native event surface requirements
- Layer split between tmux, Pi semantics, and clawhip routing
- current-vs-target comparison
- implementation gap map

### Scope out

- detailed Rust implementation design for clawhip internals
- final event payload wire format for every future event
- detailed Pi extension code
- sink-specific formatting for Telegram/Slack/Discord

---

### 2. Current execution snapshot

## What exists now

Current Pi implementation in this repo is primarily a **Layer A launcher wrapper**:

- `skills/pi/create.sh`
- `skills/pi/prompt.sh`
- `skills/pi/tail.sh`
- `plugins/pi/plugin.toml`
- `plugins/pi/bridge.sh` (placeholder)
- docs for Pi architecture / Layer A / live verification

## What the current implementation does well

- launches Pi in tmux
- keeps the session attachable and visually inspectable
- supports prompt injection into tmux
- supports output tailing
- supports stale/keyword monitoring through `clawhip tmux new`
- resolves Pi launcher sensibly:
  - explicit override
  - global `pi`
  - `pi-mono/pi-test.sh` fallback
- resolves clawhip launcher sensibly:
  - explicit override
  - `clawhip` on `PATH`
  - repo-local `cargo run -- ...`
- waits for the tmux session to exist before reporting success

## What the current implementation does not yet do

- does not consume Pi native JSON event stream
- does not consume Pi RPC events
- does not use Pi extensions/packages as a first-class event source
- does not provide turn/cycle-level semantic events
- does not provide tool-level semantic routing from Pi-native data
- does not yet provide a durable local event stream for visual/state consumers
- does not yet provide OMC-class parity beyond launcher-level operator UX

## Current architectural reality

Today’s Pi integration is best described as:

```text
tmux-first launcher wrapper with optional session emits
```

Target architecture should instead become:

```text
tmux visual surface
+ Pi native semantic event surface
+ clawhip routing/state/rendering
```

---

### 3. Proposed documentation architecture

This spec should sit alongside the existing Pi docs with clear roles:

- `docs/pi-mono-architecture-notes.md`
  - what matters about Pi’s real architecture
- `docs/pi-layer-a-operator-guide.md`
  - current tmux-first visible-session workflow
- `docs/pi-integration-plan.md`
  - phased plan / rollout order
- `docs/pi-native-parity-spec.md`
  - target-state spec + current parity comparison
- `docs/native-event-contract.md`
  - canonical cross-tool event normalization once Pi-native mapping exists

### Source-of-truth rule

For Pi work:

- **operator workflow** truth lives in `pi-layer-a-operator-guide.md`
- **phase ordering** truth lives in `pi-integration-plan.md`
- **target parity / gap analysis** truth lives in this file
- **normalized event naming** truth should eventually live in `native-event-contract.md`

---

### 4. Planned file changes

## This spec defines the following target surfaces

### Existing surfaces to keep

- `skills/pi/create.sh`
- `skills/pi/prompt.sh`
- `skills/pi/tail.sh`
- `plugins/pi/plugin.toml`
- `plugins/pi/bridge.sh`

### Surfaces likely to be upgraded later

- `plugins/pi/bridge.sh`
  - from placeholder to actual native Pi event bridge
- `docs/native-event-contract.md`
  - updated when Pi-native events are normalized into canonical clawhip events
- `tests/*`
  - once structured Pi-native ingestion exists

### Likely future additions

- `integrations/pi/` or equivalent runtime bridge module
- fixture payloads for Pi JSON/RPC event mapping
- parity verification checklist extensions in `docs/live-verification.md`

---

### 5. Draft snippets

## 5.1 Target architecture

```text
                ┌──────────────────────┐
                │  tmux session / TMS  │
                │  visual operator UI  │
                └──────────┬───────────┘
                           │
                           │ visible execution
                           ▼
                    ┌─────────────┐
                    │     Pi      │
                    │ native run  │
                    └──────┬──────┘
                           │
                 ┌─────────┼─────────┐
                 │         │         │
                 ▼         ▼         ▼
            JSON mode   RPC mode   extension/package hooks
                 └─────────┬─────────┘
                           ▼
                 ┌───────────────────┐
                 │ clawhip Pi bridge │
                 │ semantic mapping  │
                 └─────────┬─────────┘
                           ▼
                 ┌───────────────────┐
                 │ state / route /   │
                 │ render / sinks    │
                 └───────────────────┘
```

## 5.2 OMC-class parity dimensions

To say Pi native integration is "OMC-class" inside clawhip, the following dimensions should be covered.

### A. Session lifecycle parity

Target capabilities:

- session start
- session finish
- session failure
- session cancellation / abort
- session idle / blocked / waiting-for-input
- retry-needed or transient failure awareness

### B. Cycle / turn parity

Target capabilities:

- pre-cycle / pre-turn marker
- turn start
- turn end
- cycle summary surface
- question-needed / operator-needed state

### C. Tool execution parity

Target capabilities:

- tool execution start
- tool execution update / streaming progress where available
- tool execution end
- tool error visibility

### D. Operator observability parity

Target capabilities:

- attachable live tmux session
- tail / pane inspection
- session naming discipline
- stale detection
- operator-visible recovery path

### E. Routing / state parity

Target capabilities:

- local state/event consumer support
- clawhip route/filter compatibility
- structured metadata fields for repo/session/tool/run status
- downstream sink support as a later concern, not the primary contract

## 5.3 Proposed Pi-native semantic event set

This is a draft Pi-native semantic set for clawhip-side use.
It does not have to be the final normalized external contract yet.

### Session-level

- `pi.session.start`
- `pi.session.finish`
- `pi.session.fail`
- `pi.session.abort`
- `pi.session.blocked`
- `pi.session.idle`
- `pi.session.retry-needed`

### Cycle-level

- `pi.cycle.pre`
- `pi.cycle.start`
- `pi.cycle.end`
- `pi.cycle.question-needed`

### Tool-level

- `pi.tool.start`
- `pi.tool.update`
- `pi.tool.end`
- `pi.tool.error`

### Higher-order workflow

- `pi.pr.created`
- `pi.test.start`
- `pi.test.finish`
- `pi.test.fail`
- `pi.handoff.needed`

These can later map into canonical clawhip contracts such as `session.*`, `tool.*`, or other normalized families.

---

### 6. Risks and open decisions

## Risks

- over-investing in tmux scraping can delay the native semantic integration
- over-investing in sink routing too early can distort the internal event model
- Pi JSON/RPC surfaces may not expose every OMC-parity concept directly without a small bridge or extension
- introducing too many event names too early can create churn before the canonical contract stabilizes

## Open decisions

### 1. What should be considered the primary semantic source?

Options:
- JSON mode first
- RPC mode first
- extension/package bridge first
- hybrid

Current recommendation:
- **JSON mode first** for semantic ingestion
- keep tmux as the visible operator surface
- use extensions later for richer semantic fill-ins

### 2. What is the minimum OMC parity bar for “usable Pi native integration”?

Recommended minimum bar:
- session start / finish / fail
- turn/cycle visibility
- tool start / end
- blocked / idle state
- attachable tmux live session
- stale detection

### 3. Should wrapper emits survive long-term?

Recommendation:
- yes, but as compatibility/auxiliary signals only
- not as the primary semantic contract

---

### 7. Verification checklist

## Target-state verification

### A. Layer separation
- [ ] tmux remains usable as the visible live surface
- [ ] Pi native semantic data is collected separately from terminal-text keyword scraping
- [ ] sink delivery is downstream of the semantic model, not the primary model

### B. Minimum native parity
- [ ] start / finish / fail can be derived natively
- [ ] turn or cycle boundaries can be observed natively
- [ ] tool execution start/end can be observed natively
- [ ] blocked / idle / waiting states are represented or derivable

### C. OMC-class usability
- [ ] operator can attach to the live session
- [ ] operator can inspect recent output without attaching
- [ ] session naming and route metadata are stable
- [ ] clawhip can maintain state without relying only on keyword heuristics

### D. Current implementation comparison complete
- [ ] current implemented state is explicitly separated from target state
- [ ] missing parity items are called out clearly
- [ ] next implementation focus is obvious

---

## Current implementation vs target parity matrix

| Area | Target spec | Current state | Status |
|---|---|---|---|
| Visible tmux session | Required | Implemented | ✅ |
| Attach / tail operator workflow | Required | Implemented | ✅ |
| Pi launcher resolution | Global `pi` first, explicit override, dev fallback | Implemented | ✅ |
| clawhip launcher resolution | PATH first, repo fallback | Implemented | ✅ |
| False-success prevention on session creation | Required | Implemented | ✅ |
| tmux keyword monitoring | Required as Layer A fallback | Implemented | ✅ |
| tmux stale monitoring | Required as Layer A fallback | Implemented | ✅ |
| Wrapper lifecycle emits | Optional secondary signal | Implemented but endpoint/reachability still problematic in live test | ⚠️ |
| Pi JSON-mode ingestion | Required for native semantics | Not implemented | ❌ |
| Pi RPC-mode ingestion | Strong candidate for native semantics | Not implemented | ❌ |
| Pi extension/package event bridge | Optional rich native source | Not implemented | ❌ |
| Native turn/cycle awareness | OMC-class parity target | Not implemented | ❌ |
| Native tool execution awareness | OMC-class parity target | Not implemented | ❌ |
| Blocked / idle / question-needed semantics | OMC-class parity target | Not implemented | ❌ |
| Local semantic event/state surface for visual logic | Required for non-sink-first design | Not implemented | ❌ |
| Canonical normalization into clawhip event contract | Required later | Not implemented for Pi-native data | ❌ |

---

## Recommended next implementation stage

The next step should **not** be more tmux-only polishing.

The next step should be:

1. define the first Pi-native semantic source (`json` mode recommended)
2. build a tiny bridge that converts Pi-native events into a local clawhip semantic model
3. compare that bridge output against the parity matrix above
4. only then decide what should route into sinks or remain local visual/state data

In short:

```text
Current state: good Layer A launcher
Next required state: first real Pi-native semantic bridge
```