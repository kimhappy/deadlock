#!/usr/bin/env bash

set -euo pipefail

LLVM_COV_BIN="${LLVM_COV:-$(command -v llvm-cov 2>/dev/null || true)}"
LLVM_PROFDATA_BIN="${LLVM_PROFDATA:-$(command -v llvm-profdata 2>/dev/null || true)}"

if [[ -z "$LLVM_COV_BIN" || -z "$LLVM_PROFDATA_BIN" ]]; then
    echo "llvm-cov / llvm-profdata not found."
    echo "Install cargo-llvm-cov and ensure llvm-tools are available."\
    exit 1
fi

if ! command -v cargo-llvm-cov &>/dev/null && [[ ! -x "${HOME}/.cargo/bin/cargo-llvm-cov" ]]; then
    echo "cargo-llvm-cov not found. Install with:"
    echo "  cargo install cargo-llvm-cov"
    exit 1
fi

CARGO_LLVM_COV="${HOME}/.cargo/bin/cargo-llvm-cov"
if ! command -v cargo-llvm-cov &>/dev/null; then
    alias cargo-llvm-cov="$CARGO_LLVM_COV"
fi

LLVM_COV="$LLVM_COV_BIN" LLVM_PROFDATA="$LLVM_PROFDATA_BIN" \
    "${CARGO_LLVM_COV}" llvm-cov --html --output-dir coverage

echo ""
echo "Coverage report: coverage/html/index.html"
