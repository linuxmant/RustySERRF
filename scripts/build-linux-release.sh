#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Building the frontend static export"
(cd frontend && npm ci && npm run build)

echo "==> Copying the static export into crates/serrf-api/static-dist (temporarily replacing the placeholder)"
rm -rf crates/serrf-api/static-dist
mkdir -p crates/serrf-api/static-dist
cp -r frontend/out/. crates/serrf-api/static-dist/

echo "==> Building serrf-api for the host Linux target"
cargo build --release -p serrf-api --features bundled-frontend

echo "==> Restoring the committed static-dist placeholder"
git checkout -- crates/serrf-api/static-dist/
git clean -fd -- crates/serrf-api/static-dist/

mkdir -p dist
cp target/release/serrf-api dist/RustySERRF-linux
chmod +x dist/RustySERRF-linux

size=$(du -h dist/RustySERRF-linux | cut -f1)
echo "==> Done: dist/RustySERRF-linux ($size)"
echo "    Hand this file to your colleague. Running it opens http://localhost:8080 automatically."
