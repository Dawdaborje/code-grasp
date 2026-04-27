#!/usr/bin/env bash
# install_mac.sh — build release binaries and install `cg` + `code-grasp-mcp` to your PATH (macOS).
#
# MCP is stdio-based: your editor or agent spawns `code-grasp-mcp` as a subprocess.
# Indexes live at "<project>/.code-grasp/" (vectors + SQLite).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BIN_DIR="${BIN_DIR:-${HOME}/.local/bin}"

die() {
	echo "error: $*" >&2
	exit 1
}

[[ "$(uname -s)" == "Darwin" ]] || die "this script is for macOS only"

command -v cargo >/dev/null 2>&1 || die "cargo not found; install Rust from https://rustup.rs"

mkdir -p "${BIN_DIR}"

echo "==> building release from ${REPO_ROOT}"
ORT_FLAGS=()
if [[ "${CODEGRASP_ORT_DYNAMIC:-}" == "1" ]]; then
	echo "    CODEGRASP_ORT_DYNAMIC=1: build uses system ONNX (ort load-dynamic). Set ORT_DYLIB_PATH to a libonnxruntime.dylib compatible with ort 2.0 / ONNX Runtime 1.20+."
	ORT_FLAGS=(--no-default-features --features ort-dynamic)
fi
(cd "${REPO_ROOT}" && cargo build --release "${ORT_FLAGS[@]}" -p code-grasp -p code-grasp-mcp)

CG_SRC="${REPO_ROOT}/target/release/cg"
MCP_SRC="${REPO_ROOT}/target/release/code-grasp-mcp"
[[ -x "${CG_SRC}" ]] || die "missing ${CG_SRC}; build failed?"
[[ -x "${MCP_SRC}" ]] || die "missing ${MCP_SRC}; build failed?"

echo "==> installing to ${BIN_DIR}"
install -m755 "${CG_SRC}" "${BIN_DIR}/cg"
install -m755 "${MCP_SRC}" "${BIN_DIR}/code-grasp-mcp"

echo ""
echo "Installed:"
echo "  ${BIN_DIR}/cg"
echo "  ${BIN_DIR}/code-grasp-mcp"
echo ""
echo "Add to PATH if needed (zsh):"
echo "  export PATH=\"${BIN_DIR}:\$PATH\""
echo ""
echo "MCP: configure your client to run \"${BIN_DIR}/code-grasp-mcp\" (stdio)."
echo ""
echo "Tip: after \`git pull\`, run this script again so PATH binaries match the tree."
echo ""
echo "Optional NVIDIA GPU: build with ort-cuda (see README). Apple Silicon uses CPU ONNX unless you add a CoreML path in the future."
