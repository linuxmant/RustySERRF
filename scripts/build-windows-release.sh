#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required (cross uses it to cross-compile for Windows). Install Docker and try again." >&2
  exit 1
fi

if ! command -v cross >/dev/null 2>&1; then
  echo "cross is required. Install it with:" >&2
  echo "  cargo install cross --git https://github.com/cross-rs/cross" >&2
  exit 1
fi

echo "==> Building the frontend static export"
(cd frontend && npm ci && npm run build)

echo "==> Copying the static export into crates/serrf-api/static-dist (temporarily replacing the placeholder)"
rm -rf crates/serrf-api/static-dist
mkdir -p crates/serrf-api/static-dist
cp -r frontend/out/. crates/serrf-api/static-dist/

echo "==> Cross-compiling serrf-api for x86_64-pc-windows-gnu"
cross build --release --target x86_64-pc-windows-gnu -p serrf-api --features bundled-frontend

echo "==> Restoring the committed static-dist placeholder"
git checkout -- crates/serrf-api/static-dist/
git clean -fd -- crates/serrf-api/static-dist/

mkdir -p dist
cp target/x86_64-pc-windows-gnu/release/serrf-api.exe dist/RustySERRF.exe

size=$(du -h dist/RustySERRF.exe | cut -f1)
echo "==> Done: dist/RustySERRF.exe ($size)"
echo "    Hand this file to your colleague. Double-clicking it opens http://localhost:8080 automatically."
