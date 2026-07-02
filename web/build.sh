#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
cargo run --manifest-path ../Cargo.toml --features dev-tools --bin generate-web-examples
wasm-pack build --target web --out-dir pkg
npm ci
npm run build
