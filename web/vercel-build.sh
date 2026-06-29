#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v rustc &>/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
fi

rustup target add wasm32-unknown-unknown

if ! command -v wasm-pack &>/dev/null; then
  curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
fi

wasm-pack build --target web --out-dir pkg
npm run build
