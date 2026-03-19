# Pi Layer A Operator Guide

This guide documents the recommended first-step Pi × clawhip integration model: a visible tmux-first workflow where Pi remains directly observable during execution.

## 1. What Layer A means

Layer A means:

- Pi runs in its normal interactive mode
- Pi lives inside a tmux session
- clawhip monitors that tmux session
- operators can attach, inspect, steer, and debug the live session
- clawhip delivers side-channel notifications to Discord/Slack

This is the right starting point when the main goal is to visualize the ongoing process instead of only consuming structured machine-readable events.

## 2. Why Layer A first

Layer A is preferred when you care about:

- seeing the live Pi interface
- keeping the operator in control
- observing tool usage and terminal behavior directly
- debugging real execution state
- avoiding hidden automation at the start

Compared to JSON or RPC integration, tmux-first operation is less structured but more observable.

## 3. Mental model

```text
operator <-> tmux <-> Pi interactive session
                      |
                      v
                  clawhip monitor
                      |
                      v
              Discord / Slack notifications
```

Pi remains the live work surface.
clawhip adds monitoring and delivery, not control-plane ownership.

## 4. Core workflow

### 4.1 Start a monitored Pi session

Use the Pi skill wrapper:

```bash
cd skills/pi
./create.sh <session-name> <worktree-path> [prompt] [channel-id] [mention]
```

Example:

```bash
./create.sh issue-123 ~/my-project/worktrees/issue-123
```

With an initial prompt:

```bash
./create.sh issue-123 ~/my-project/worktrees/issue-123 "Fix the failing tests and create a PR"
```

With explicit channel and mention:

```bash
./create.sh issue-123 ~/my-project/worktrees/issue-123 "Fix the failing tests and create a PR" 1234567890 "<@user-id>"
```

### 4.2 Attach to the live session

```bash
tmux attach -t issue-123
```

This is the main visualization path.

### 4.3 Send a new prompt to a running session

```bash
cd skills/pi
./prompt.sh issue-123 "Stop and investigate the failing integration test first"
```

### 4.4 Inspect recent output without attaching

```bash
cd skills/pi
./tail.sh issue-123
./tail.sh issue-123 50
```

## 5. What clawhip contributes in Layer A

In this model, clawhip should focus on four things:

### 5.1 Session launch ergonomics

clawhip can create/register the tmux session consistently and keep route/channel parameters attached to the session.

### 5.2 Keyword alerts

The tmux monitor watches for important phrases such as:

- `error`
- `FAILED`
- `panic`
- `PR created`
- `complete`
- `done`

These are best treated as operator hints, not canonical workflow state.

### 5.3 Stale-session alerts

If Pi stops producing output for too long, clawhip can notify the target channel that the session may be stuck or waiting.

### 5.4 Delivery and mention policy

clawhip owns:

- Discord/Slack target resolution
- mention policy
- formatting
- notification transport

## 6. Recommended session naming

Use session names that help both humans and route rules.

Good examples:

- `issue-123`
- `repo-main`
- `feature-auth`
- `bugfix-cache-invalidations`

Avoid opaque names like:

- `test1`
- `work`
- `abc`

The session name is the main operator handle for:

- tmux attach
- tmux tail
- alert interpretation
- future route filtering

## 7. Recommended defaults for Pi Layer A

### Keywords

Suggested default keyword set:

```text
error,Error,FAILED,PR created,panic,complete,done
```

These are intentionally pragmatic and output-oriented.
They should be revised after observing real Pi session transcripts.

### Stale timeout

Suggested initial timeout:

```text
30 minutes
```

Lower values may be too noisy for long model thinking or slow tool runs.

## 8. Operational expectations

Layer A is not trying to fully understand Pi state semantically.

It is trying to make ongoing sessions:

- visible
- inspectable
- interruptible
- easier to notice from chat channels

That means:

- tmux is the source of truth for live execution
- clawhip alerts are secondary guidance
- keyword alerts are advisory, not authoritative

## 9. Limitations of Layer A

Layer A is intentionally simple, but it has limits:

- keyword matches are brittle
- notification quality depends on Pi output wording
- clawhip cannot distinguish all semantic states from terminal text alone
- blocked vs thinking vs waiting may not always be obvious from output

These are acceptable trade-offs for the first phase because the main goal is session visibility.

## 10. How Layer A should evolve

Once Layer A is stable, the next step should be to add a structured bridge without removing tmux visibility.

Recommended next move:

```text
Pi in tmux for visibility + Pi JSON/RPC bridge for structured state
```

That would let clawhip keep:

- live observable sessions
- better route fidelity
- better lifecycle notifications

## 11. Verification checklist

Use this checklist when validating a Layer A setup:

- [ ] Pi launches in a named tmux session
- [ ] `tmux attach -t <session>` works
- [ ] `skills/pi/prompt.sh` can steer the session
- [ ] `skills/pi/tail.sh` shows recent output
- [ ] keyword alerts reach the target channel
- [ ] stale alerts reach the target channel
- [ ] the target channel sees enough signal without being too noisy
- [ ] an operator can understand how to reattach and inspect the session quickly

## 12. Recommended related docs

- `docs/pi-mono-architecture-notes.md`
- `docs/pi-integration-plan.md`
- `skills/pi/SKILL.md`
