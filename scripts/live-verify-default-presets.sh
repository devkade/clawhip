#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<USAGE
Usage:
  scripts/live-verify-default-presets.sh issue-opened
  scripts/live-verify-default-presets.sh issue-comment
  scripts/live-verify-default-presets.sh issue-closed
  scripts/live-verify-default-presets.sh pr-opened
  scripts/live-verify-default-presets.sh pr-merged
  scripts/live-verify-default-presets.sh tmux-keyword
  scripts/live-verify-default-presets.sh tmux-stale
  scripts/live-verify-default-presets.sh tmux-wrapper
  scripts/live-verify-default-presets.sh pi-local

Required env vars for GitHub/Discord verification:
  CLAWHIP_REPO           e.g. Yeachan-Heo/clawhip
  CLAWHIP_CHANNEL        Discord test channel id
  CLAWHIP_BOT_TOKEN      Discord bot token
  CLAWHIP_DAEMON_URL     e.g. http://127.0.0.1:25294
Optional:
  CLAWHIP_MENTION        mention tag to assert in messages

Local Pi verification env vars:
  CLAWHIP_DAEMON_URL     e.g. http://127.0.0.1:25294
  PI_WORKDIR             e.g. ~/projects/pi-mono/packages/coding-agent
  CLAWHIP_BIN            optional explicit clawhip launcher path/script
  CLAWHIP_PI_BIN         optional explicit pi launcher path
USAGE
}

mode=${1:-}
[[ -n "$mode" ]] || { usage; exit 1; }

require_common() {
  : "${CLAWHIP_REPO:?set CLAWHIP_REPO}"
  : "${CLAWHIP_CHANNEL:?set CLAWHIP_CHANNEL}"
  : "${CLAWHIP_BOT_TOKEN:?set CLAWHIP_BOT_TOKEN}"
  : "${CLAWHIP_DAEMON_URL:?set CLAWHIP_DAEMON_URL}"
}

fetch_messages() {
  curl -fsS \
    -H "Authorization: Bot $CLAWHIP_BOT_TOKEN" \
    -H 'Content-Type: application/json' \
    "https://discord.com/api/v10/channels/$CLAWHIP_CHANNEL/messages?limit=20"
}

assert_message_contains() {
  local needle="$1"
  local mention="${CLAWHIP_MENTION:-}"
  python3 - "$needle" "$mention" <<'PY'
import json, sys
needle = sys.argv[1]
mention = sys.argv[2]
msgs = json.load(sys.stdin)
for msg in msgs:
    content = msg.get('content', '')
    if needle in content and (not mention or mention in content):
        print(content)
        raise SystemExit(0)
raise SystemExit(1)
PY
}

case "$mode" in
  issue-opened)
    require_common
    echo "Create a real issue in $CLAWHIP_REPO, then confirm Discord delivery."
    echo "Example: gh issue create --repo $CLAWHIP_REPO --title 'clawhip live issue-opened <ts>' --body 'verification'"
    ;;
  issue-comment)
    require_common
    echo "Add a real comment to an existing open issue in $CLAWHIP_REPO, then confirm Discord delivery."
    ;;
  issue-closed)
    require_common
    echo "Close a real issue in $CLAWHIP_REPO, then confirm Discord delivery."
    ;;
  pr-opened)
    require_common
    echo "Open a real PR in $CLAWHIP_REPO from a temporary branch/base branch, then confirm Discord delivery."
    ;;
  pr-merged)
    require_common
    echo "Merge a real temporary PR in $CLAWHIP_REPO, then confirm Discord delivery."
    ;;
  tmux-keyword)
    require_common
    echo "Create or use a monitored tmux session and print a configured keyword (e.g. error / PR created), then confirm Discord delivery."
    ;;
  tmux-stale)
    require_common
    echo "Create or use a monitored tmux session, let it go stale past the configured threshold, then confirm Discord delivery."
    ;;
  tmux-wrapper)
    require_common
    echo "Run clawhip tmux new ... with keywords/mention/channel and verify wrapper-generated delivery in Discord."
    ;;
  pi-local)
    : "${CLAWHIP_DAEMON_URL:?set CLAWHIP_DAEMON_URL}"
    : "${PI_WORKDIR:?set PI_WORKDIR}"
    session="pi-verify-$(date +%s)"
    repo_root="$(cd "$(dirname "$0")/.." && pwd)"
    echo "Launching Pi local validation session: $session"
    (
      cd "$repo_root/skills/pi"
      ./create.sh "$session" "$PI_WORKDIR"
    )
    sleep 5
    echo
    echo "GET $CLAWHIP_DAEMON_URL/api/pi/state"
    curl -fsS "$CLAWHIP_DAEMON_URL/api/pi/state"
    echo
    echo
    echo "GET $CLAWHIP_DAEMON_URL/api/pi/render-summary/compact"
    curl -fsS "$CLAWHIP_DAEMON_URL/api/pi/render-summary/compact"
    echo
    ;;
  *)
    usage
    exit 1
    ;;
esac

if [[ "$mode" != "pi-local" ]]; then
  echo
  echo "Recent Discord messages for channel $CLAWHIP_CHANNEL:"
  fetch_messages | python3 -c 'import json,sys; msgs=json.load(sys.stdin); print(json.dumps(msgs[:5], indent=2)[:4000])'

  echo
  echo "To assert a concrete message after performing the live action, pipe the same message list into assert_message_contains manually or extend this script with an operation-specific needle."
fi
