#!/usr/bin/env bash
# Fast local build: debug WASM, skip npm ci, regenerate examples only when fixtures change.
set -euo pipefail
cd "$(dirname "$0")"

ROOT="$(cd .. && pwd)"

needs_examples() {
  [[ ! -f src/examples.js ]] && return 0
  [[ "$ROOT/tests/fixtures.toml" -nt src/examples.js ]] && return 0
  find "$ROOT/tests/fixtures" -type f -newer src/examples.js -print -quit | grep -q .
}

if needs_examples; then
  cargo run --manifest-path "$ROOT/Cargo.toml" --features dev-tools --bin generate-web-examples
fi

wasm-pack build --dev --target web --out-dir pkg

if [[ ! -d node_modules ]]; then
  npm install
fi
npm run build
