#!/usr/bin/env bash
set -euo pipefail

# Runs Cargo strictly using the project-local isolated toolchain in .toolchain/.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLCHAIN_DIR="${REPO_ROOT}/.toolchain"

export RUSTUP_HOME="${TOOLCHAIN_DIR}/rustup"
export CARGO_HOME="${TOOLCHAIN_DIR}/cargo"
export PATH="${CARGO_HOME}/bin:${PATH}"

if [[ ! -x "${CARGO_HOME}/bin/cargo" ]]; then
  echo "error: isolated toolchain not installed in ${TOOLCHAIN_DIR}" >&2
  echo "hint: run './scripts/setup-env.sh' first." >&2
  exit 1
fi

exec "${CARGO_HOME}/bin/cargo" "$@"
