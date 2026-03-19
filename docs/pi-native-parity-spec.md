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
- **Pi TUI in tmux** remains the primary runtime and operator surface,
- clawhip builds observability and state collection around that live session,
- optional structured Pi surfaces (`json`, `rpc`, extension/package hooks) may enrich the model later,
- and the total capability level should reach **OMC-class operational completeness**.

In short:

```text
Pi TUI in tmux
+ clawhip-side observability around the live session
+ OMC-class lifecycle / operator / routing completeness
```

### Scope in

- target functional parity areas relative to OMC-class usage
- TUI/tmux-first Pi observability requirements
- optional later use of Pi native structured surfaces
- layer split between tmux, Pi runtime, and clawhip routing/state
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

- does not define a TUI-first observability contract beyond launcher-level behavior
- does not provide turn/cycle-level semantic state from the live TUI session
- does not provide tool-level semantic routing from a Pi-native runtime signal
- does not yet provide a durable local event/state surface for visual/state consumers
- does not yet provide OMC-class parity beyond launcher-level operator UX
- does not yet decide clearly which states come from TUI observation vs optional auxiliary sources

## Current architectural reality

Today’s Pi integration is best described as:

```text
tmux-first launcher wrapper with optional session emits
```

Target architecture should instead become:

```text
Pi TUI in tmux
+ clawhip-side observability/state collection around the live session
+ optional auxiliary semantic enrichment later
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
  - canonical cross-tool event normalization once optional Pi-native mappings exist

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
  - from placeholder to an actual observability/state bridge where needed
- `docs/native-event-contract.md`
  - updated only when optional structured Pi-native mappings become stable
- `tests/*`
  - once semantic ingestion/state handling exists beyond launcher level

### Likely future additions

- `integrations/pi/` or equivalent runtime bridge module
- fixtures for TUI-state/semantic inference or optional JSON/RPC enrichment
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
                           │ live visible execution
                           ▼
                    ┌─────────────┐
                    │   Pi TUI    │
                    │ primary run │
                    └──────┬──────┘
                           │
                           ▼
                 ┌───────────────────┐
                 │ clawhip Pi layer  │
                 │ observability +   │
                 │ state collection  │
                 └─────────┬─────────┘
                           ▼
                 ┌───────────────────┐
                 │ state / route /   │
                 │ render / sinks    │
                 └───────────────────┘

      Optional later auxiliary sources:
      - Pi JSON mode
      - Pi RPC mode
      - Pi extension/package hooks
```

## 5.2 Why this matches clawhip better

This TUI-first direction is more consistent with existing clawhip behavior because clawhip already leans toward:

- tmux-monitored operator workflows
- visible long-running sessions
- wrapper/session lifecycle awareness
- notification/routing as a layer around execution rather than a replacement for execution

So for Pi, a TUI/tmux-first design is not a deviation from clawhip.
It is a closer match to how clawhip already works with OMC/OMX-style monitored sessions.

The main difference is:

- OMC/OMX currently rely more on wrapper emits and normalized contract payloads
- Pi should eventually reach similar operational richness while still respecting Pi’s native runtime style

## 5.3 OMC-class parity dimensions

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

## 5.4 Proposed Pi semantic state set

This is a draft TUI-first semantic set for clawhip-side use.
It does not require Pi JSON/RPC to be the primary runtime path.

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

- over-investing in tmux keyword scraping can delay better state semantics
- over-investing in sink routing too early can distort the internal event model
- Pi JSON/RPC surfaces may still be useful later, but forcing them too early can break the TUI-first operator goal
- introducing too many event names too early can create churn before the state model stabilizes

## Open decisions

### 1. What should be considered the primary runtime source?

Options:
- TUI/tmux first
- JSON mode first
- RPC mode first
- extension/package bridge first
- hybrid

Current recommendation:
- **TUI/tmux first** as the primary runtime and visualization surface
- build clawhip-side observability around the live session first
- use JSON/RPC/extensions later as auxiliary semantic fill-ins where they add value

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
- not as the primary runtime model

---

### 7. Verification checklist

## Target-state verification

### A. Layer separation
- [ ] tmux remains the usable live surface
- [ ] Pi TUI remains the primary runtime path
- [ ] optional auxiliary signals stay supplemental rather than becoming the default runtime mode
- [ ] sink delivery is downstream of the state model, not the primary model

### B. Minimum native parity
- [ ] start / finish / fail can be observed or inferred reliably
- [ ] turn or cycle boundaries can be observed or inferred reliably
- [ ] tool activity can be observed or inferred reliably
- [ ] blocked / idle / waiting states are represented or derivable

### C. OMC-class usability
- [ ] operator can attach to the live session
- [ ] operator can inspect recent output without attaching
- [ ] session naming and route metadata are stable
- [ ] clawhip can maintain state without relying only on brittle heuristics

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
| tmux keyword monitoring | Required as TUI-first fallback/auxiliary signal | Implemented | ✅ |
| tmux stale monitoring | Required as TUI-first fallback/auxiliary signal | Implemented | ✅ |
| Wrapper lifecycle emits | Optional secondary signal | Implemented but endpoint/reachability still problematic in live test | ⚠️ |
| TUI-first observability contract | Required | Not implemented as a formal contract yet | ❌ |
| Native turn/cycle awareness | OMC-class parity target | Not implemented | ❌ |
| Native tool execution awareness | OMC-class parity target | Not implemented | ❌ |
| Blocked / idle / question-needed semantics | OMC-class parity target | Not implemented | ❌ |
| Local semantic event/state surface for visual logic | Required for non-sink-first design | Not implemented | ❌ |
| Optional JSON-mode enrichment | Later auxiliary source | Not implemented | ◻️ |
| Optional RPC-mode enrichment | Later auxiliary source | Not implemented | ◻️ |
| Optional extension/package enrichment | Later auxiliary source | Not implemented | ◻️ |
| Canonical normalization into clawhip event contract | Required later | Not implemented for Pi-native semantics | ❌ |

---

## Recommended next execution stage

The next step should be:

1. define the **TUI/tmux-first observability contract** for Pi sessions
2. decide which session states are collected from the live visible session itself
3. add auxiliary semantic sources only where TUI/tmux observation is insufficient
4. then decide what should route into sinks or remain local visual/state data

In short:

```text
Current state: good Layer A launcher
Next required state: TUI-first Pi observability contract
```
