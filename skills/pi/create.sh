#!/bin/bash
# clawhip × Pi — Create a monitored Pi tmux session
# Usage: create.sh <session-name> <worktree-path> [prompt] [channel-id] [mention]

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
PI_BIN="${CLAWHIP_PI_BIN:-pi}"

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

PROJECT="${CLAWHIP_PI_PROJECT:-$(detect_project)}"

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

quote() {
  printf '%q' "$1"
}

PI_CMD=$(cat <<EOF
source ~/.zshrc
START_TS=\$(date +%s)
cleanup() {
  local exit_code=\$?
  local elapsed=\$(( \$(date +%s) - START_TS ))
  if [ "\$exit_code" -eq 0 ]; then
    clawhip emit session.finished --tool pi --tool_name pi --session_name $(quote "$SESSION") --session_id $(quote "$SESSION") --repo_name $(quote "$PROJECT") --repo_path $(quote "$WORKDIR") --command $(quote "$PI_BIN $PI_FLAGS") --elapsed "\$elapsed" --summary "Pi session finished"$EMIT_SUFFIX || true
  else
    clawhip emit session.failed --tool pi --tool_name pi --session_name $(quote "$SESSION") --session_id $(quote "$SESSION") --repo_name $(quote "$PROJECT") --repo_path $(quote "$WORKDIR") --command $(quote "$PI_BIN $PI_FLAGS") --elapsed "\$elapsed" --error "exit \$exit_code" --summary "Pi session failed"$EMIT_SUFFIX || true
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM
clawhip emit session.started --tool pi --tool_name pi --session_name $(quote "$SESSION") --session_id $(quote "$SESSION") --repo_name $(quote "$PROJECT") --repo_path $(quote "$WORKDIR") --command $(quote "$PI_BIN $PI_FLAGS") --summary "Pi session started"$EMIT_SUFFIX || true
${PI_ENV:+$PI_ENV }$PI_BIN $PI_FLAGS
EOF
)

ARGS+=(-- "$PI_CMD")

nohup clawhip "${ARGS[@]}" &>/dev/null &

echo "✓ Created visible Pi session: $SESSION in $WORKDIR (clawhip monitored)"
echo "  Project: $PROJECT"
echo "  Tool:    $PI_BIN"
echo "  Attach:  tmux attach -t $SESSION"
echo "  Tail:    $(dirname "$0")/tail.sh $SESSION"
echo "  Notes:   tmux is the primary live view; clawhip alerts are secondary"

if [ -n "$PROMPT" ]; then
  sleep 10
  tmux send-keys -t "$SESSION" -l "$PROMPT"
  tmux send-keys -t "$SESSION" Enter
  echo "  Prompt: sent literal text after 10s init delay"
fi
