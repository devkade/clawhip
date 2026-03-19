# Pi Mono Architecture Notes for clawhip Integration

This document captures the parts of `pi-mono` that matter for clawhip integration work.

## 1. Purpose

The goal of this note is to prevent clawhip-side Pi integration work from assuming that Pi is just another OMC/OMX-style wrapper target.

Pi has its own architecture, its own extensibility model, and multiple machine-readable integration surfaces. Those should shape clawhip design decisions.

## 2. Repo-level structure

The monorepo root describes Pi as a toolkit for building AI agents and managing LLM deployments. The main integration target for clawhip is not the whole repo; it is primarily the coding-agent package.

Important packages:

- `packages/ai` — model/provider abstraction layer
- `packages/agent` — agent runtime and event types
- `packages/coding-agent` — the actual `pi` CLI and coding harness
- `packages/tui` — terminal UI layer
- `packages/mom` — Slack-oriented harness built on top of Pi

## 3. Primary clawhip integration target

For clawhip compatibility, the key package is:

- `packages/coding-agent`

That package owns:

- the `pi` CLI binary
- interactive mode
- JSON mode
- RPC mode
- SDK/session integration
- resource loading for skills/extensions/prompts/themes

## 4. Pi design philosophy that affects clawhip

Pi is deliberately extensible-first.

The README explicitly frames Pi as a minimal coding harness that should be adapted to the user's workflow via:

- extensions
- skills
- prompt templates
- themes
- Pi packages

That means clawhip integration should avoid assuming that Pi wants a hardcoded built-in notification workflow.

The cleaner model is:

- Pi exposes state and extensibility surfaces
- clawhip consumes or complements those surfaces
- clawhip owns routing, formatting, mentions, and delivery policy

## 5. Integration surfaces Pi already provides

Pi already exposes several surfaces that matter for clawhip.

### 5.1 Interactive mode

Interactive mode is the visible operator-first UI. This is the best match for users who want to:

- watch the session live
- attach/detach from an ongoing run
- steer the agent manually
- inspect the real terminal state rather than only derived events

This is why Layer A should be tmux-first.

### 5.2 JSON event stream mode

Pi supports:

```bash
pi --mode json
```

This emits structured events to stdout. Important event families include:

- `agent_start`
- `agent_end`
- `turn_start`
- `turn_end`
- `message_start`
- `message_update`
- `message_end`
- `tool_execution_start`
- `tool_execution_update`
- `tool_execution_end`
- `auto_compaction_start`
- `auto_compaction_end`
- `auto_retry_start`
- `auto_retry_end`

This is the best near-term structured bridge surface for Layer B or later native clawhip integration.

### 5.3 RPC mode

Pi supports:

```bash
pi --mode rpc
```

RPC mode provides:

- command/response protocol over stdin/stdout
- streaming agent events
- state inspection
- session control
- extension UI request/response support

This is the strongest process-integration surface when clawhip or another orchestrator wants deep control.

### 5.4 SDK / embedding

Pi can also be embedded via its TypeScript/Node SDK using `AgentSession` and related APIs.

This is the cleanest long-term integration path if clawhip ever wants a deep in-process adapter, but it is not the simplest first step.

### 5.5 Extensions

Pi extensions can:

- register commands
- register tools
- subscribe to lifecycle events
- emit custom internal events
- show UI / notifications
- alter workflow behavior

This matters because Pi-native clawhip compatibility could eventually be implemented as:

- a Pi extension that emits structured clawhip-compatible events, or
- a Pi package that bundles the integration

## 6. Why Layer A is the right first step

The current user goal is session visualization and operator awareness during the run.

For that goal, the best primary integration surface is not JSON mode or RPC mode.
It is:

- Pi interactive mode in tmux
- clawhip tmux monitoring and notifications on top

Why:

- tmux preserves a visible live session
- humans can attach at any time
- the session remains operator-first
- clawhip can still provide channel notifications, stale alerts, and keyword alerts

So the correct first model is:

```text
Pi interactive mode -> tmux session -> clawhip tmux monitoring -> Discord/Slack notifications
```

## 7. Layer model for Pi × clawhip

### Layer A — visible operator workflow

Primary goal:

- let humans see and control the session as it happens

Recommended surface:

- Pi interactive mode in tmux
- `clawhip tmux new` / `clawhip tmux watch`

Primary clawhip role:

- observability
- channel notifications
- stale detection
- keyword alerts

### Layer B — structured bridge workflow

Primary goal:

- let clawhip consume structured machine-readable Pi state

Recommended surfaces:

- `pi --mode json`
- `pi --mode rpc`
- future Pi extension/package bridge

Primary clawhip role:

- translate Pi native events into canonical `session.*`
- route by `tool = "pi"`, `session_name`, `repo_name`, etc.

## 8. Important design constraint

Do not treat Pi as just a renamed OMC/OMX target.

OMC/OMX integration patterns are still useful for:

- wrapper ergonomics
- tmux launching
- prompt injection
- operator docs

But Pi has richer native integration surfaces. Those should inform the long-term design even if Layer A ships first.

## 9. Practical conclusion for current clawhip work

For the current phase:

- optimize Pi support for Layer A first
- keep the session visible in tmux
- document attach/tail/watch workflows clearly
- treat keyword/stale detection as the initial clawhip value-add

For later phases:

- add a JSON-mode bridge
- map Pi native events into clawhip `session.*`
- optionally build a Pi extension or package for direct event emission

## 10. Recommended next docs to keep aligned

When Pi integration evolves, keep these clawhip docs aligned:

- `docs/pi-integration-plan.md`
- `docs/pi-layer-a-operator-guide.md`
- `docs/native-event-contract.md`
- `docs/live-verification.md`
