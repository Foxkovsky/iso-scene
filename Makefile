# iso-scene — Makefile
#
# Everything runs through cargo; the Makefile is a convenience layer for the
# commands used most often.

CARGO ?= cargo

.PHONY: help fmt fmt-check check clippy test bench doc ci run clean vscode-test

help:
	@printf '%s\n' \
		'make fmt        format the workspace' \
		'make check      cargo check --workspace --all-targets' \
		'make clippy     clippy with warnings as errors' \
		'make test       run all tests' \
		'make bench      run the criterion benchmarks' \
		'make vscode-test test the VS Code extension (needs node)' \
		'make doc        build documentation' \
		'make ci         fmt-check + clippy + test + doc' \
		'make run        run the CLI (pass ARGS=...)' \
		'make clean      remove target/'

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all --check

check:
	$(CARGO) check --workspace --all-targets

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

doc:
	$(CARGO) doc --workspace --no-deps

bench:
	$(CARGO) bench --workspace

vscode-test:
	$(CARGO) build --release -p scene-cli
	node --test editors/vscode/test

ci: fmt-check clippy test doc

run:
	$(CARGO) run -p scene-cli -- $(ARGS)

clean:
	$(CARGO) clean
