# Pi TUI Observability Contract

## Purpose

This document defines the **TUI/tmux-first observability contract** for Pi inside clawhip.

It answers one specific question:

> If Pi runs in its normal TUI mode inside tmux, what states and signals must clawhip be able to observe, infer, store, and optionally route in order to reach OMC-class operational usefulness?

This document does **not** make JSON/RPC the primary runtime path.
Those remain optional later enrichment sources.

---

## 1. Core design rule

The primary runtime model is:

```text
Pi TUI in tmux
-> visible operator session
-> clawhip-side observability/state collection
-> optional routing/rendering/sinks
```

That implies:

- **Pi TUI** is the real execution surface
- **tmux** is the visibility and control surface
- **clawhip** is the observability/state/routing layer around the live session
- **Telegram/Slack/Discord** are downstream outputs, not the primary contract

---

## 2. Source hierarchy

For Pi state collection, clawhip should use this priority order.

### Primary source
- tmux-visible Pi TUI session

### Secondary sources
- wrapper lifecycle emits
- pane snapshots / pane tail reads
- tmux session metadata

### Optional later auxiliary sources
- Pi JSON mode
- Pi RPC mode
- Pi extension/package hooks

### Rule

The system must remain useful even if only the **primary source** exists.
Auxiliary sources may improve precision, but should not be required for the basic operator workflow.

---

## 3. Required observable state classes

## 3.1 Session state

These are the minimum session-level states clawhip should support.

### Required
- `session.created`
- `session.attached-available`
- `session.running`
- `session.finished`
- `session.failed`
- `session.aborted`
- `session.stale`

### Notes
- `session.created` means tmux session registered successfully
- `session.attached-available` means operator can attach via tmux
- `session.running` means the session exists and Pi is active or plausibly active
- `session.stale` is time-based and may overlap with running/blocking uncertainty

## 3.2 Activity state

These describe whether the live session appears active.

### Required
- `activity.active`
- `activity.idle`
- `activity.blocked-or-waiting`

### Inference guidance
- `activity.active` may be inferred from visible output churn or recent pane updates
- `activity.idle` may be inferred from lack of pane changes for a shorter threshold than stale
- `activity.blocked-or-waiting` may be inferred from stable phrases/prompts or long-lived paused states

### Important constraint
- `blocked-or-waiting` may begin as a best-effort inference, not a guaranteed semantic truth

## 3.3 Cycle state

To approach OMC-class usefulness, clawhip should eventually reason about user-visible “work cycles” even if Pi does not expose a perfect native cycle marker in TUI mode.

### Target states
- `cycle.pre`
- `cycle.start`
- `cycle.end`
- `cycle.question-needed`

### Current expectation
These may begin as inferred states rather than guaranteed native states.

## 3.4 Tool state

To approach OMC-class observability, clawhip should eventually expose tool-level visibility.

### Target states
- `tool.start`
- `tool.update`
- `tool.end`
- `tool.error`

### Current expectation
These are not required for the first launcher phase, but they are part of the parity target.

---

## 4. Required operator capabilities

These are not optional. If these are missing, the TUI-first contract is not satisfied.

### Required
- launch Pi in a named tmux session
- attach to the session
- inspect recent output without attaching
- inject prompts into the running session
- detect stale sessions
- maintain stable session naming for routing and operator handling

### Current implementation status
- launch: implemented
- attach: implemented by tmux session creation
- inspect recent output: implemented via `skills/pi/tail.sh`
- prompt injection: implemented via `skills/pi/prompt.sh`
- stale detection: implemented through `clawhip tmux new`
- naming: implemented structurally, but naming policy may still need refinement

---

## 5. State derivation rules

This section defines where each class of state should come from.

| State class | Preferred source | Fallback source | Notes |
|---|---|---|---|
| session.created | tmux session exists | wrapper launch success path | must not rely on optimistic printouts only |
| session.running | tmux session exists + session script active | recent pane activity | coarse but acceptable early on |
| session.finished | process exit / wrapper lifecycle | tmux session disappearance after clean completion | early implementation may remain approximate |
| session.failed | wrapper lifecycle / shell exit | pane-visible error + abnormal session end | secondary emit path currently unstable |
| session.stale | tmux stale timer | none | already a strong fit for clawhip |
| activity.active | pane updates | keyword churn | output-based inference |
| activity.idle | no pane changes for short interval | none | separate from stale |
| activity.blocked-or-waiting | stable waiting prompt phrases / long pause | manual operator inference | initially heuristic |
| cycle.* | inferred from visible workflow markers | optional later auxiliary source | not implemented yet |
| tool.* | inferred from visible tool activity | optional later auxiliary source | not implemented yet |

---

## 6. What must be true to call this OMC-class

Pi does not need to copy OMC internals.
But to reach **OMC-class operational usefulness** in clawhip, all of the following must become true.

### Required outcome set
- operator can reliably launch, attach, steer, and inspect Pi sessions
- clawhip can tell when a session exists, is stale, and has probably ended or failed
- clawhip can maintain meaningful session state beyond simple keyword alerts
- clawhip can expose enough state for visual logic / chanMS / higher layers to consume
- optional sinks remain downstream consumers, not the primary truth source

### Stretch outcome set
- cycle-awareness
- tool-awareness
- blocked/question-needed semantics
- native semantic enrichment from auxiliary sources

---

## 7. Current implementation comparison

### Already implemented
- named tmux session launch
- attachable live session
- tail helper
- prompt injection helper
- stale monitoring
- keyword monitoring
- launcher hardening (Pi resolution, clawhip resolution, false-success prevention)

### Partially implemented
- wrapper lifecycle emits (`session.started`, `session.finished`, `session.failed`)
  - present in design and code
  - runtime reachability still unstable in live testing

### Not implemented
- formal local state model for TUI-observed Pi sessions
- activity state derivation beyond stale/keyword
- cycle-level observability
- tool-level observability
- blocked/question-needed state derivation
- local event/state surface for visual logic consumers

---

## 8. Recommended next implementation order

### Step 1
Define a small internal state model for TUI-observed Pi sessions.

Minimum first version:
- created
- running
- stale
- finished
- failed
- active
- idle

### Step 2
Decide where that state lives.

Candidate forms:
- local in-memory clawhip state
- JSONL event log
- session-scoped state file

### Step 3
Add activity derivation around pane changes rather than only keyword/stale checks.

### Step 4
Add blocked/question-needed heuristics only after the basic state model is stable.

### Step 5
Only then decide whether auxiliary JSON/RPC enrichment is worth adding.

---

## 9. Verification checklist

- [ ] launching Pi creates a real tmux session
- [ ] operator can attach immediately
- [ ] `tail.sh` reflects recent pane output
- [ ] `prompt.sh` can inject a new instruction
- [ ] stale detection works without auxiliary sources
- [ ] session existence is treated as a real state, not optimistic wrapper output
- [ ] a minimum internal state model is defined for TUI-observed sessions
- [ ] the design remains useful even without JSON/RPC

---

## 10. Relationship to other docs

- `docs/pi-layer-a-operator-guide.md` explains operator workflow
- `docs/pi-integration-plan.md` explains rollout order
- `docs/pi-native-parity-spec.md` explains target parity and gap analysis
- this document explains the **runtime observability contract** for the TUI-first model
 this contract
- this document explains the **runtime observability contract** for the TUI-first model
