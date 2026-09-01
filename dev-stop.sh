#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [ ! -f .dev.pid ]; then
  echo "Nothing to stop (.dev.pid not found)." >&2
  exit 1
fi

read -r API_PID FRONTEND_PID < .dev.pid
kill "$API_PID" 2>/dev/null || true
kill -- "-$FRONTEND_PID" 2>/dev/null || true
rm .dev.pid
echo "Stopped serrf-api (pid $API_PID) and frontend (pid $FRONTEND_PID)."
