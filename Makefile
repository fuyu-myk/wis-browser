.PHONY: help dev build build_frontend check test fmt fmt-check lint clean install

ARCH   := $(shell uname -m)
OS_RAW := $(shell uname -s)

ifeq ($(OS_RAW),Darwin)
  ifeq ($(ARCH),arm64)
    TARGET_TRIPLE ?= aarch64-apple-darwin
  else
    TARGET_TRIPLE ?= x86_64-apple-darwin
  endif
else ifeq ($(OS_RAW),Linux)
  TARGET_TRIPLE ?= x86_64-unknown-linux-gnu
else
  TARGET_TRIPLE ?= x86_64-pc-windows-msvc
endif

## install: Install frontend dependencies
install:
	cd crates/shell && pnpm install

## dev: Launch the Tauri app in development mode
dev:
	cd crates/shell && pnpm tauri dev

## build: Production build of the Tauri app
build:
	cd crates/shell && pnpm tauri build --target $(TARGET_TRIPLE)

## build_frontend: Build only the React frontend bundle
build_frontend:
	cd crates/shell && pnpm install && pnpm build

## check: cargo check the entire workspace
check:
	cargo check --workspace --all-targets

## test: Run the full workspace test suite
test:
	cargo test --workspace --all-targets

## fmt: Format all Rust code in the workspace
fmt:
	cargo fmt --all

## fmt-check: Verify Rust code is formatted (CI gate)
fmt-check:
	cargo fmt --all -- --check

## lint: Run clippy with warnings as errors
lint:
	cargo clippy --workspace --all-targets -- -D warnings

## clean: Remove build artefacts
clean:
	cargo clean
	rm -rf crates/shell/dist crates/shell/node_modules

## help: Display this help message
help:
	@grep -E '^## ' Makefile | sed 's/## //'
