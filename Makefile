# CodeGrasp — common Cargo workflows (GNU Make).

.PHONY: default help build check test fmt fmt-check clippy doc doc-open clean install ci core-features

default: help

help:
	@echo "Targets:"
	@echo "  build        cargo build --workspace"
	@echo "  check        cargo check --workspace"
	@echo "  test         cargo test --workspace"
	@echo "  fmt          cargo fmt (write)"
	@echo "  fmt-check    cargo fmt --check"
	@echo "  clippy       clippy with -D warnings (all targets)"
	@echo "  doc          rustdoc for cg_core + cg_proto (--no-deps)"
	@echo "  doc-open     same as doc, then open in browser"
	@echo "  clean        cargo clean"
	@echo "  install      install cg and code-grasp-mcp (--path)"
	@echo "  ci           fmt-check + clippy + test (merge gate)"
	@echo "  core-features  build cg_core with lsp + openai"

build:
	cargo build --workspace

check:
	cargo check --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

doc:
	cargo doc -p cg_core -p cg_proto --no-deps

doc-open:
	cargo doc -p cg_core -p cg_proto --no-deps --open

clean:
	cargo clean

install:
	cargo install --path cg_cli --locked
	cargo install --path cg_mcp --locked

ci: fmt-check clippy test

core-features:
	cargo build -p cg_core --features "lsp,openai"
