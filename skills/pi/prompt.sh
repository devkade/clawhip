#!/bin/bash
# clawhip × Pi — Send a prompt to an existing Pi session
# Usage: prompt.sh <session-name> "<prompt-text>"

set -euo pipefail

SESSION="${1:?Usage: $0 <session-name> \"<prompt-text>\"}"
PROMPT="${2:?Usage: $0 <session-name> \"<prompt-text>\"}"

if ! tmux has-session -t "$SESSION" 2>/dev/null; then
  echo "❌ Session not found: $SESSION"
  exit 1
fi

tmux send-keys -t "$SESSION" -l "$PROMPT"
tmux send-keys -t "$SESSION" Enter

echo "✓ Sent to $SESSION (unverified): ${PROMPT:0:80}..."
