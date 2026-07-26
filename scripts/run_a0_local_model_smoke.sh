#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

CARGO_BIN="${CARGO_BIN:-cargo}"
SMOKE_FEATURES="${BENSHU_A0_SMOKE_FEATURES:-llama_cpp rocm}"

cd "${REPO_ROOT}"

echo "[A0 smoke] repo: ${REPO_ROOT}"
echo "[A0 smoke] features: ${SMOKE_FEATURES}"

exec "${CARGO_BIN}" test -p benshu-inference test_a0_model_profile_smoke \
  --test smoke_test --features "${SMOKE_FEATURES}" -- --nocapture
