#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [ -f .dev.pid ]; then
  echo "Already running (see .dev.pid). Run ./dev-stop.sh first." >&2
  exit 1
fi

cleanup_and_fail() {
  local message="$1"
  local log_file="$2"
  echo "$message" >&2
  echo "--- tail of $log_file ---" >&2
  tail -n 20 "$log_file" >&2 2>/dev/null || true
  kill "$API_PID" 2>/dev/null || true
  kill -- "-$FRONTEND_PID" 2>/dev/null || true
  exit 1
}

echo "Starting serrf-api..."
cargo run -p serrf-api > /tmp/serrf-api.log 2>&1 &
API_PID=$!

echo "Starting Next.js dev server..."
setsid bash -c 'cd frontend && exec npm run dev' > /tmp/serrf-frontend.log 2>&1 &
FRONTEND_PID=$!

echo "Waiting for serrf-api to come up..."
api_ready=false
for _ in $(seq 1 60); do
  if ! kill -0 "$API_PID" 2>/dev/null; then
    break
  fi
  if curl -sf --max-time 2 http://127.0.0.1:8080/health > /dev/null 2>&1; then
    api_ready=true
    break
  fi
  sleep 1
done
if [ "$api_ready" != true ]; then
  cleanup_and_fail "serrf-api failed to start." /tmp/serrf-api.log
fi

echo "Waiting for frontend to come up..."
frontend_ready=false
for _ in $(seq 1 60); do
  if ! kill -0 "$FRONTEND_PID" 2>/dev/null; then
    break
  fi
  if curl -sf --max-time 2 -o /dev/null http://127.0.0.1:3000 2>&1; then
    frontend_ready=true
    break
  fi
  sleep 1
done
if [ "$frontend_ready" != true ]; then
  cleanup_and_fail "frontend failed to start." /tmp/serrf-frontend.log
fi

echo "$API_PID $FRONTEND_PID" > .dev.pid
echo "serrf-api (pid $API_PID) logging to /tmp/serrf-api.log"
echo "frontend (pid $FRONTEND_PID) logging to /tmp/serrf-frontend.log"
echo "Run ./dev-stop.sh to stop both."
