#!/usr/bin/env bash
# install_linux.sh — build release binaries and install `cg` + `code-grasp-mcp` to your PATH.
#
# MCP is stdio-based: your editor or agent spawns `code-grasp-mcp` as a subprocess (no systemd).
# Each tool call passes a project `path`; indexes live at "<path>/.code-grasp/" (vectors + SQLite).
#
# Older installs may have created ~/.config/systemd/user/code-grasp-mcp.service — remove it and run
# `systemctl --user daemon-reload` if you no longer want that unit.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BIN_DIR="${BIN_DIR:-${HOME}/.local/bin}"

die() {
	echo "error: $*" >&2
	exit 1
}

[[ "$(uname -s)" == "Linux" ]] || die "this script is for Linux only"

command -v cargo >/dev/null 2>&1 || die "cargo not found; install Rust from https://rustup.rs"

mkdir -p "${BIN_DIR}"

echo "==> building release from ${REPO_ROOT}"
ORT_FLAGS=()
if [[ "${CODEGRASP_ORT_DYNAMIC:-}" == "1" ]]; then
	echo "    CODEGRASP_ORT_DYNAMIC=1: build uses system ONNX (ort load-dynamic). Set ORT_DYLIB_PATH to a libonnxruntime.so compatible with ort 2.0 / ONNX Runtime 1.20+."
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
echo "Add to PATH if needed, e.g. in ~/.profile:"
echo "  export PATH=\"${BIN_DIR}:\$PATH\""
echo ""
echo "MCP: configure your client to run \"${BIN_DIR}/code-grasp-mcp\" (stdio). Do not use systemd for this."
echo ""
echo "Tip: after \`git pull\` or other repo updates, run this script again so \`cg\` and \`code-grasp-mcp\` on your PATH match the current tree (or use \`cargo run -p code-grasp -- …\` from the clone)."
echo ""
echo "If \`cg index\` dies with a segmentation fault during embedding init (common with bundled ORT on some Linux setups), rebuild with: \`CODEGRASP_ORT_DYNAMIC=1 bash scripts/install_linux.sh\` and set ORT_DYLIB_PATH to a suitable libonnxruntime.so."
