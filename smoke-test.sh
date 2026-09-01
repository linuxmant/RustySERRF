#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cleanup() {
  ./dev-stop.sh || true
}
trap cleanup EXIT

./dev-start.sh

echo "Waiting for serrf-api..."
for _ in $(seq 1 30); do
  if curl -sf http://127.0.0.1:8080/health > /dev/null; then
    break
  fi
  sleep 1
done
curl -sf http://127.0.0.1:8080/health | grep -q "^ok$" || { echo "serrf-api /health check failed" >&2; exit 1; }

echo "Waiting for frontend..."
for _ in $(seq 1 60); do
  if curl -sf -o /dev/null http://127.0.0.1:3000; then
    break
  fi
  sleep 1
done
curl -sf -o /dev/null http://127.0.0.1:3000 || { echo "frontend / check failed" >&2; exit 1; }

echo "Smoke test passed: both processes are up and answering."
