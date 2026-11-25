#!/usr/bin/env bash
set -euo pipefail

# Idempotent, project-local environment bootstrap for LangFlip.
# Installs rustup/rustc/cargo strictly under <repo>/.toolchain/ without
# modifying ~/.rustup, ~/.cargo, ~/.profile, or ~/.bashrc.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPS_CONF="${REPO_ROOT}/toolchain/deps.conf"

if [[ ! -f "${DEPS_CONF}" ]]; then
  echo "error: missing dependency manifest: ${DEPS_CONF}" >&2
  exit 1
fi

# shellcheck source=../toolchain/deps.conf
source "${DEPS_CONF}"

MODE="bootstrap"
INSTALL_SYSTEM_DEPS=0

for arg in "$@"; do
  case "${arg}" in
    --check)
      MODE="check"
      ;;
    --install-system-deps)
      INSTALL_SYSTEM_DEPS=1
      ;;
    -h|--help)
      cat <<'USAGE'
Usage: ./scripts/setup-env.sh [--check] [--install-system-deps]

Options:
  --check                 Audit host dependencies and isolated toolchain without installing.
  --install-system-deps   Install any missing host packages via the detected package manager
                          before bootstrapping the isolated Rust toolchain in .toolchain/.
USAGE
      exit 0
      ;;
    *)
      echo "error: unknown option: ${arg}" >&2
      exit 2
      ;;
  esac
done

detect_pkg_mgr() {
  if command -v apt-get >/dev/null 2>&1; then
    echo "apt"
  elif command -v dnf >/dev/null 2>&1; then
    echo "dnf"
  elif command -v pacman >/dev/null 2>&1; then
    echo "pacman"
  else
    echo "unknown"
  fi
}

PKG_MGR="$(detect_pkg_mgr)"
MISSING_BINS=()
MISSING_PKGS=()

echo "==> Checking declared host dependencies (${DEPS_CONF})"
for entry in "${HOST_DEPS[@]}"; do
  IFS='|' read -r bin apt_pkg dnf_pkg pacman_pkg purpose <<< "${entry}"
  if command -v "${bin}" >/dev/null 2>&1; then
    printf "  [ok]      %-26s (%s)\n" "${bin}" "${purpose}"
  else
    printf "  [missing] %-26s (%s)\n" "${bin}" "${purpose}"
    MISSING_BINS+=("${bin}")
    case "${PKG_MGR}" in
      apt)    MISSING_PKGS+=("${apt_pkg}") ;;
      dnf)    MISSING_PKGS+=("${dnf_pkg}") ;;
      pacman) MISSING_PKGS+=("${pacman_pkg}") ;;
      *)      MISSING_PKGS+=("${apt_pkg}") ;;
    esac
  fi
done

TOOLCHAIN_DIR="${REPO_ROOT}/.toolchain"
export RUSTUP_HOME="${TOOLCHAIN_DIR}/rustup"
export CARGO_HOME="${TOOLCHAIN_DIR}/cargo"
export PATH="${CARGO_HOME}/bin:${PATH}"

echo "==> Checking isolated Rust toolchain (${TOOLCHAIN_DIR})"
if [[ -x "${CARGO_HOME}/bin/rustup" && -x "${CARGO_HOME}/bin/cargo" ]]; then
  printf "  [ok]      isolated rustup/cargo present in %s\n" "${TOOLCHAIN_DIR}"
  TOOLCHAIN_PRESENT=1
else
  printf "  [missing] isolated rustup/cargo not yet installed in %s\n" "${TOOLCHAIN_DIR}"
  TOOLCHAIN_PRESENT=0
fi

if [[ "${MODE}" == "check" ]]; then
  if [[ ${#MISSING_BINS[@]} -gt 0 || ${TOOLCHAIN_PRESENT} -eq 0 ]]; then
    echo ""
    echo "Audit incomplete: missing ${#MISSING_BINS[@]} host tool(s), toolchain_present=${TOOLCHAIN_PRESENT}."
    if [[ ${#MISSING_PKGS[@]} -gt 0 ]]; then
      echo "Run './scripts/setup-env.sh --install-system-deps' to install missing host packages and bootstrap .toolchain/."
    else
      echo "Run './scripts/setup-env.sh' to bootstrap .toolchain/."
    fi
    exit 1
  fi
  echo "All declared dependencies and isolated toolchain are ready."
  exit 0
fi

if [[ ${#MISSING_BINS[@]} -gt 0 ]]; then
  if [[ ${INSTALL_SYSTEM_DEPS} -eq 1 ]]; then
    echo "==> Installing missing host packages via ${PKG_MGR}: ${MISSING_PKGS[*]}"
    case "${PKG_MGR}" in
      apt)
        sudo apt-get update && sudo apt-get install -y "${MISSING_PKGS[@]}"
        ;;
      dnf)
        sudo dnf install -y "${MISSING_PKGS[@]}"
        ;;
      pacman)
        sudo pacman -Sy --noconfirm "${MISSING_PKGS[@]}"
        ;;
      *)
        echo "error: unsupported package manager; install manually: ${MISSING_PKGS[*]}" >&2
        exit 1
        ;;
    esac
  else
    echo "warning: missing host tools: ${MISSING_BINS[*]}" >&2
    echo "hint: run './scripts/setup-env.sh --install-system-deps' to install: ${MISSING_PKGS[*]}" >&2
    if ! command -v curl >/dev/null 2>&1 || ! command -v sha256sum >/dev/null 2>&1; then
      echo "error: curl and sha256sum are required to bootstrap rustup." >&2
      exit 1
    fi
  fi
fi

mkdir -p "${RUSTUP_HOME}" "${CARGO_HOME}" "${TOOLCHAIN_DIR}/tmp"

if [[ ! -x "${CARGO_HOME}/bin/rustup" ]]; then
  echo "==> Downloading pinned rustup-init ${RUSTUP_VERSION} (${RUSTUP_HOST_TRIPLE})"
  INIT_BIN="${TOOLCHAIN_DIR}/tmp/rustup-init"
  INIT_SHA="${TOOLCHAIN_DIR}/tmp/rustup-init.sha256"
  curl -fsSL "${RUSTUP_INIT_URL}" -o "${INIT_BIN}"

  ACTUAL_SHA="$(sha256sum "${INIT_BIN}" | awk '{print $1}')"
  if [[ -n "${RUSTUP_INIT_SHA256}" ]]; then
    EXPECTED_SHA="${RUSTUP_INIT_SHA256}"
  else
    curl -fsSL "${RUSTUP_INIT_SHA256_URL}" -o "${INIT_SHA}"
    EXPECTED_SHA="$(awk '{print $1}' "${INIT_SHA}")"
    echo "info: verified against official .sha256; pin in toolchain/deps.conf:"
    echo "      RUSTUP_INIT_SHA256=\"${EXPECTED_SHA}\""
  fi

  if [[ "${ACTUAL_SHA}" != "${EXPECTED_SHA}" ]]; then
    echo "error: rustup-init SHA-256 mismatch!" >&2
    echo "  expected: ${EXPECTED_SHA}" >&2
    echo "  actual:   ${ACTUAL_SHA}" >&2
    rm -f "${INIT_BIN}" "${INIT_SHA}"
    exit 1
  fi

  chmod +x "${INIT_BIN}"
  echo "==> Installing rustup strictly into ${TOOLCHAIN_DIR} (--no-modify-path)"
  "${INIT_BIN}" -y --no-modify-path --default-toolchain none
  rm -f "${INIT_BIN}" "${INIT_SHA}"
fi

echo "==> Ensuring pinned Rust toolchain from rust-toolchain.toml is installed"
(cd "${REPO_ROOT}" && rustup show active-toolchain >/dev/null 2>&1 || rustup install)

if [[ -f "${REPO_ROOT}/Cargo.toml" ]]; then
  echo "==> Pre-fetching Cargo workspace dependencies into ${CARGO_HOME}"
  (cd "${REPO_ROOT}" && cargo fetch)
fi

echo "==> Isolated environment ready."
echo "    RUSTUP_HOME=${RUSTUP_HOME}"
echo "    CARGO_HOME=${CARGO_HOME}"
echo "    Use './scripts/cargo.sh <args>' to run cargo commands."
