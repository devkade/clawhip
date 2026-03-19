# Pi Session State Model

## Purpose

This document defines the first internal state model for **TUI-observed Pi sessions** inside clawhip.

It turns the TUI observability contract into a concrete state object and transition model that clawhip can implement incrementally.

This model is intentionally:

- TUI/tmux-first
- small enough to ship early
- useful without JSON/RPC
- extensible later when richer Pi-native signals are added

---

## 1. Design goals

The first state model must satisfy these constraints:

- represent real operator-visible session facts
- avoid pretending to know more than the live session actually reveals
- separate **fact**, **inference**, and **heuristic confidence**
- remain useful even if wrapper emits fail
- be easy to map into routing, rendering, and future sink logic

---

## 2. State model overview

Each Pi session should have one primary state object.

## 2.1 Canonical identity fields

These fields identify the session.

```json
{
  "tool": "pi",
  "sessionName": "issue-123",
  "sessionId": "issue-123",
  "project": "my-repo",
  "repoPath": "/path/to/worktree",
  "branch": "issue-123",
  "tmuxSession": "issue-123"
}
```

### Notes
- `sessionId` may equal `sessionName` initially
- `branch` is optional in the first phase
- `tmuxSession` should remain explicit even if it matches the session name

---

## 2.2 Minimum runtime state object

```json
{
  "tool": "pi",
  "sessionName": "issue-123",
  "sessionId": "issue-123",
  "project": "my-repo",
  "repoPath": "/path/to/worktree",
  "tmuxSession": "issue-123",

  "lifecycle": "running",
  "activity": "active",
  "stale": false,
  "attachable": true,

  "createdAt": 1773940000,
  "updatedAt": 1773940123,
  "lastPaneChangeAt": 1773940120,
  "lastPromptInjectAt": null,
  "lastObservedText": "…",

  "exitCode": null,
  "failureReason": null,

  "confidence": {
    "lifecycle": "medium",
    "activity": "low"
  },

  "sources": {
    "tmuxExists": true,
    "wrapperEmit": false,
    "paneObserved": true,
    "auxiliary": false
  }
}
```

---

## 3. State dimensions

## 3.1 Lifecycle dimension

This tracks coarse session progress.

### Allowed values
- `created`
- `running`
- `finished`
- `failed`
- `aborted`
- `unknown`

### Semantics
- `created` = tmux session exists and launch succeeded, but active execution is not yet confirmed
- `running` = session is plausibly active or at least still alive
- `finished` = session ended successfully or is believed to have cleanly completed
- `failed` = session ended abnormally or a failure condition is strongly indicated
- `aborted` = session appears intentionally terminated
- `unknown` = insufficient signal to classify confidently

## 3.2 Activity dimension

This tracks operator-relevant liveness within a running session.

### Allowed values
- `active`
- `idle`
- `blocked-or-waiting`
- `unknown`

### Semantics
- `active` = recent pane change or visible progress
- `idle` = alive but quiet for a short threshold
- `blocked-or-waiting` = visible prompt/wait pattern or long pause suggesting operator input is needed
- `unknown` = insufficient evidence

## 3.3 Staleness dimension

### Allowed values
- `true`
- `false`

### Semantics
- `true` = stale threshold exceeded
- `false` = stale threshold not exceeded

Staleness is orthogonal to lifecycle and activity.
A session may be:
- `running` + `stale=true`
- `running` + `activity=blocked-or-waiting`

---

## 4. Confidence model

The model should explicitly capture confidence so clawhip does not confuse inferred state with guaranteed truth.

### Allowed confidence values
- `high`
- `medium`
- `low`

### Suggested use
- `high` = derived from strong signal like tmux existence + clean wrapper exit code
- `medium` = derived from stable multi-signal inference
- `low` = derived from heuristic text or timing only

### Rule

Operator-facing rendering may simplify confidence, but internal state should keep it.

---

## 5. Evidence sources

The state model must track where state came from.

### Minimum evidence flags

```json
{
  "sources": {
    "tmuxExists": true,
    "paneObserved": true,
    "wrapperEmit": false,
    "auxiliary": false
  }
}
```

### Meaning
- `tmuxExists` = tmux session existence check contributed
- `paneObserved` = pane content or pane change timing contributed
- `wrapperEmit` = wrapper lifecycle signal contributed
- `auxiliary` = JSON/RPC/extension source contributed

This lets later debugging answer:

- did the system infer this from pane text?
- did the wrapper explicitly emit it?
- was this only a timer-based guess?

---

## 6. Minimum field set for v1

The first shippable model should include only these fields.

| Field | Required | Notes |
|---|---|---|
| `tool` | yes | always `pi` |
| `sessionName` | yes | operator-facing handle |
| `sessionId` | yes | may equal sessionName initially |
| `project` | yes | detected or configured |
| `repoPath` | yes | worktree/repo path |
| `tmuxSession` | yes | explicit tmux name |
| `lifecycle` | yes | coarse state |
| `activity` | yes | active/idle/blocked/unknown |
| `stale` | yes | boolean |
| `attachable` | yes | usually derived from tmux existence |
| `createdAt` | yes | first known timestamp |
| `updatedAt` | yes | last state recompute time |
| `lastPaneChangeAt` | no | recommended |
| `lastPromptInjectAt` | no | useful for later heuristics |
| `exitCode` | no | when available |
| `failureReason` | no | when available |
| `confidence` | yes | lifecycle + activity minimum |
| `sources` | yes | evidence tracking |

---

## 7. Transition model

## 7.1 Required transitions

### Launch path
- `unknown -> created`
- `created -> running`

### Normal execution path
- `running -> finished`

### Failure path
- `running -> failed`

### Abort path
- `running -> aborted`

### Quiet path
- `running + active -> running + idle`
- `running + idle -> running + blocked-or-waiting`
- `running + any -> stale=true`

## 7.2 Transition constraints

- lifecycle transitions should be monotonic when possible
- `finished`, `failed`, and `aborted` should be terminal states
- activity may continue to change while lifecycle is `running`
- stale may flip on/off while lifecycle remains `running` if the implementation allows recovery

---

## 8. Transition derivation guidance

| Transition | Preferred trigger | Fallback trigger |
|---|---|---|
| `unknown -> created` | tmux session exists after launch | wrapper reports launch success |
| `created -> running` | pane shows Pi process presence or updates | session exists for a grace period |
| `running -> finished` | wrapper clean exit | session disappears without strong failure evidence |
| `running -> failed` | wrapper non-zero exit | visible failure text + abnormal session end |
| `running -> aborted` | operator kill signal known | session killed externally |
| `active -> idle` | no pane changes for short threshold | none |
| `idle -> blocked-or-waiting` | waiting-style pane text or prolonged pause | manual/operator hint |
| `* -> stale=true` | stale timer threshold exceeded | none |

---

## 9. Suggested storage shapes

The implementation may use one or more of these.

### Option A — in-memory session state
Best for:
- live routing
- low overhead

Weakness:
- less durable for audits/debugging

### Option B — JSONL event log
Best for:
- debugging
- replay
- future state recomputation

Weakness:
- requires more plumbing

### Option C — session-scoped state file
Best for:
- simple persistence
- human inspectability

Weakness:
- less expressive than an event log

### Recommendation
Start with:
- in-memory state for runtime behavior
- optional JSONL event log for debugging/replay

---

## 10. State update cadence

The model should update from three classes of stimuli.

### Event-driven
- tmux session creation
- prompt injection
- wrapper exit

### Poll-driven
- tmux session existence checks
- pane change detection
- stale timer checks

### Optional later auxiliary
- JSON/RPC/extension-derived signals

---

## 11. Explicit non-goals for v1

The first state model should **not** try to do all of this yet:

- perfect semantic understanding of every Pi screen
- perfect cycle detection
- exact tool boundary capture
- sink-specific message formatting logic
- complete PR/test/handoff workflow modeling

Those belong in later iterations.

---

## 12. Mapping to the observability contract

This model is the first concrete implementation candidate for:

- `docs/pi-tui-observability-contract.md`

It operationalizes these contract requirements:
- session state
- activity state
- staleness
- attachability
- minimum operator usefulness

---

## 13. Recommended next implementation step

Implement the v1 state object with:

1. session existence checks
2. pane-change timestamps
3. idle/stale derivation
4. optional wrapper-exit enrichment

The file-by-file implementation plan now lives in:

- `docs/pi-session-state-implementation-plan.md`

After that:
- add blocked/waiting heuristics
- then evaluate whether auxiliary JSON/RPC enrichment is still needed for missing parity
