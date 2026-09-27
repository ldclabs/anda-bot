BUILD_ENV := rust

# Match CI: embedded Brain's unoptimized async frames need a larger test stack.
RUST_MIN_STACK ?= 16777216

.PHONY: lint fix test

lint:
	@cargo fmt
	@cargo clippy --all-targets --all-features

fix:
	@cargo fmt --all
	@cargo clippy --fix --workspace --tests

test:
	@RUST_MIN_STACK=$(RUST_MIN_STACK) cargo test --workspace --all-features -- --nocapture
