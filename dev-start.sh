#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [ -f .dev.pid ]; then
  echo "Already running (see .dev.pid). Run ./dev-stop.sh first." >&2
  exit 1
fi

echo "Starting serrf-api..."
cargo run -p serrf-api > /tmp/serrf-api.log 2>&1 &
API_PID=$!

echo "Starting Next.js dev server..."
setsid bash -c 'cd frontend && exec npm run dev' > /tmp/serrf-frontend.log 2>&1 &
FRONTEND_PID=$!

echo "$API_PID $FRONTEND_PID" > .dev.pid
echo "serrf-api (pid $API_PID) logging to /tmp/serrf-api.log"
echo "frontend (pid $FRONTEND_PID) logging to /tmp/serrf-frontend.log"
echo "Run ./dev-stop.sh to stop both."
