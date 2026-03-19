#!/bin/bash
# clawhip × Pi — Create a monitored Pi tmux session
# Usage: create.sh <session-name> <worktree-path> [prompt] [channel-id] [mention]
# Layer A intent: keep Pi visible in tmux; clawhip monitoring/notifications are secondary.

set -euo pipefail

SESSION="${1:?Usage: $0 <session-name> <worktree-path> [prompt] [channel-id] [mention]}"
WORKDIR="${2:?Usage: $0 <session-name> <worktree-path> [prompt] [channel-id] [mention]}"
PROMPT="${3:-}"
CHANNEL="${4:-}"
MENTION="${5:-}"

KEYWORDS="${CLAWHIP_PI_KEYWORDS:-error,Error,FAILED,PR created,panic,complete,done}"
STALE_MIN="${CLAWHIP_PI_STALE_MIN:-30}"
PI_FLAGS="${CLAWHIP_PI_FLAGS:-}"
PI_ENV="${CLAWHIP_PI_ENV:-}"
PI_BIN_OVERRIDE="${CLAWHIP_PI_BIN:-}"
PROMPT_DELAY="${CLAWHIP_PI_PROMPT_DELAY:-10}"
CLAWHIP_BIN_OVERRIDE="${CLAWHIP_BIN:-}"

if [ ! -d "$WORKDIR" ]; then
  echo "❌ Directory not found: $WORKDIR"
  exit 1
fi

detect_project() {
  local common_dir
  common_dir="$(git -C "$WORKDIR" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
  if [ -n "$common_dir" ]; then
    basename "$(dirname "$common_dir")"
  else
    basename "$WORKDIR"
  fi
}

find_pi_mono_root() {
  local dir="$WORKDIR"
  while [ "$dir" != "/" ]; do
    if [ -x "$dir/pi-test.sh" ] && [ -f "$dir/packages/coding-agent/src/cli.ts" ]; then
      printf '%s\n' "$dir"
      return 0
    fi
    dir="$(dirname "$dir")"
  done
  return 1
}

resolve_pi_command() {
  if [ -n "$PI_BIN_OVERRIDE" ]; then
    printf '%s\n' "$PI_BIN_OVERRIDE"
    return 0
  fi

  local pi_mono_root
  pi_mono_root="$(find_pi_mono_root || true)"
  if [ -n "$pi_mono_root" ]; then
    printf '%s\n' "$pi_mono_root/pi-test.sh"
    return 0
  fi

  if command -v pi >/dev/null 2>&1; then
    printf '%s\n' "$(command -v pi)"
    return 0
  fi

  return 1
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
clawhip_repo_root="$(cd "$script_dir/../.." && pwd)"

resolve_clawhip_command() {
  if [ -n "$CLAWHIP_BIN_OVERRIDE" ]; then
    CLAWHIP_CMD=("$CLAWHIP_BIN_OVERRIDE")
    return 0
  fi

  if command -v clawhip >/dev/null 2>&1; then
    CLAWHIP_CMD=("$(command -v clawhip)")
    return 0
  fi

  if command -v cargo >/dev/null 2>&1 && [ -f "$clawhip_repo_root/Cargo.toml" ] && [ -f "$clawhip_repo_root/src/main.rs" ]; then
    CLAWHIP_CMD=(cargo run --quiet --manifest-path "$clawhip_repo_root/Cargo.toml" --)
    return 0
  fi

  return 1
}

quote() {
  printf '%q' "$1"
}

shell_join() {
  local out=""
  local part
  for part in "$@"; do
    printf -v out '%s%q ' "$out" "$part"
  done
  printf '%s' "${out% }"
}

PROJECT="${CLAWHIP_PI_PROJECT:-$(detect_project)}"
PI_CMD_PATH="$(resolve_pi_command || true)"

if [ -z "$PI_CMD_PATH" ]; then
  echo "❌ Could not find Pi executable. Set CLAWHIP_PI_BIN or run from inside a pi-mono checkout with pi-test.sh present."
  exit 1
fi

CLAWHIP_CMD=()
resolve_clawhip_command || {
  echo "❌ Could not find clawhip launcher. Set CLAWHIP_BIN, install clawhip, or run from inside a clawhip repo with cargo available."
  exit 1
}
CLAWHIP_SHELL_CMD="$(shell_join "${CLAWHIP_CMD[@]}")"

ARGS=(
  tmux new
  -s "$SESSION"
  -c "$WORKDIR"
  --keywords "$KEYWORDS"
  --stale-minutes "$STALE_MIN"
)

[ -n "$CHANNEL" ] && ARGS+=(--channel "$CHANNEL")
[ -n "$MENTION" ] && ARGS+=(--mention "$MENTION")

EMIT_ARGS=()
[ -n "$CHANNEL" ] && EMIT_ARGS+=(--channel "$CHANNEL")
[ -n "$MENTION" ] && EMIT_ARGS+=(--mention "$MENTION")
EMIT_SUFFIX=""
if [ ${#EMIT_ARGS[@]} -gt 0 ]; then
  printf -v EMIT_SUFFIX ' %q' "${EMIT_ARGS[@]}"
fi

DISPLAY_CMD="$PI_CMD_PATH${PI_FLAGS:+ $PI_FLAGS}"
SESSION_SCRIPT="$(mktemp "/tmp/clawhip-pi-${SESSION}.XXXXXX.sh")"
chmod +x "$SESSION_SCRIPT"
cat > "$SESSION_SCRIPT" <<EOF
#!/bin/bash
set -euo pipefail
cleanup() {
  local exit_code=\$?
  local elapsed=\$(( \$(date +%s) - START_TS ))
  if [ "\$exit_code" -eq 0 ]; then
    $CLAWHIP_SHELL_CMD emit session.finished --tool pi --tool_name pi --session_name $(quote "$SESSION") --session_id $(quote "$SESSION") --repo_name $(quote "$PROJECT") --repo_path $(quote "$WORKDIR") --command $(quote "$DISPLAY_CMD") --elapsed "\$elapsed" --summary "Pi session finished"$EMIT_SUFFIX || true
  else
    $CLAWHIP_SHELL_CMD emit session.failed --tool pi --tool_name pi --session_name $(quote "$SESSION") --session_id $(quote "$SESSION") --repo_name $(quote "$PROJECT") --repo_path $(quote "$WORKDIR") --command $(quote "$DISPLAY_CMD") --elapsed "\$elapsed" --error "exit \$exit_code" --summary "Pi session failed"$EMIT_SUFFIX || true
  fi
  rm -f "$SESSION_SCRIPT"
}
trap cleanup EXIT
trap 'exit 130' INT TERM
START_TS=\$(date +%s)
$CLAWHIP_SHELL_CMD emit session.started --tool pi --tool_name pi --session_name $(quote "$SESSION") --session_id $(quote "$SESSION") --repo_name $(quote "$PROJECT") --repo_path $(quote "$WORKDIR") --command $(quote "$DISPLAY_CMD") --summary "Pi session started"$EMIT_SUFFIX || true
${PI_ENV:+$PI_ENV }$(quote "$PI_CMD_PATH") $PI_FLAGS
EOF

ARGS+=(-- "$SESSION_SCRIPT")

nohup "${CLAWHIP_CMD[@]}" "${ARGS[@]}" &>/dev/null &

for _ in 1 2 3 4 5 6 7 8 9 10; do
  if tmux has-session -t "$SESSION" 2>/dev/null; then
    break
  fi
  sleep 0.5
done

if ! tmux has-session -t "$SESSION" 2>/dev/null; then
  echo "❌ Failed to create tmux session: $SESSION"
  echo "   Tried clawhip launcher: $CLAWHIP_SHELL_CMD"
  rm -f "$SESSION_SCRIPT"
  exit 1
fi

echo "✓ Created visible Pi session: $SESSION in $WORKDIR (clawhip monitored)"
echo "  Project: $PROJECT"
echo "  Command: $DISPLAY_CMD"
echo "  Clawhip: $CLAWHIP_SHELL_CMD"
echo "  Attach:  tmux attach -t $SESSION"
echo "  Tail:    $(dirname "$0")/tail.sh $SESSION"
echo "  Notes:   tmux is the primary live view; clawhip alerts are secondary"

if [ -n "$PROMPT" ]; then
  sleep "$PROMPT_DELAY"
  tmux send-keys -t "$SESSION" -l "$PROMPT"
  tmux send-keys -t "$SESSION" Enter
  echo "  Prompt: sent literal text after ${PROMPT_DELAY}s init delay"
fi
