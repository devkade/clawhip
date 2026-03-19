# clawhip × Pi

Launch Pi coding sessions with automatic clawhip notifications.

## What you get

- Preferred `session.*` wrapper emits for Pi lifecycle events
- `tool = "pi"` metadata for stable route filtering
- Session keyword alerts (error, failed, PR created, complete, etc.)
- Stale session detection (no output for N minutes)
- All notifications routed by clawhip to the correct Discord/Slack sink
- tmux remains available as a fallback/operator monitoring layer

## Prerequisites

- [clawhip](https://github.com/Yeachan-Heo/clawhip) installed and daemon running
- Pi installed and available on `PATH`
- tmux

## Usage

### Create a session

```bash
./create.sh <session-name> <worktree-path> [prompt] [channel-id] [mention]
```

```bash
# Basic — uses clawhip default channel
./create.sh issue-123 ~/my-project/worktrees/issue-123

# Start a session and auto-send an initial prompt after the TUI initializes
./create.sh issue-123 ~/my-project/worktrees/issue-123 "Fix the bug in src/main.rs and create a PR to dev"

# With prompt, specific channel, and mention
./create.sh issue-123 ~/my-project/worktrees/issue-123 "Fix the bug in src/main.rs and create a PR to dev" 1234567890 "<@user-id>"
```

`create.sh` launches Pi in a clawhip-monitored tmux session and emits `session.started`, `session.finished`, and `session.failed` directly from the shell wrapper. If you pass a prompt, the script waits 10 seconds for Pi to initialize, then sends the prompt via `tmux send-keys -l` before pressing Enter.

### Send a prompt

```bash
./prompt.sh <session-name> "Fix the bug in src/main.rs and create a PR to dev"
```

`prompt.sh` sends prompt text in tmux literal mode (`send-keys -l`) and presses Enter separately so quotes, punctuation, and leading dashes are preserved exactly.

### Monitor output

```bash
./tail.sh <session-name> [lines]
```

## Customization

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CLAWHIP_PI_KEYWORDS` | `error,Error,FAILED,PR created,panic,complete,done` | Comma-separated keywords to monitor |
| `CLAWHIP_PI_STALE_MIN` | `30` | Minutes before stale alert |
| `CLAWHIP_PI_FLAGS` | *(empty)* | Extra flags passed to `pi` |
| `CLAWHIP_PI_ENV` | *(empty)* | Extra env vars prepended to the Pi command |
| `CLAWHIP_PI_PROJECT` | detected from the git common dir (fallback: worktree name) | Override the project name sent in lifecycle events |
| `CLAWHIP_PI_BIN` | `pi` | Override the Pi executable |

### Route example

```toml
[[routes]]
event = "session.*"
filter = { tool = "pi" }
channel = "1234567890"
mention = "<@your-user-id>"
format = "compact"
```
