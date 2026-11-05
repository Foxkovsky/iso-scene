# iso-scene — Makefile
#
# Everything runs through cargo; the Makefile is a convenience layer for the
# commands used most often.

CARGO ?= cargo

.PHONY: help fmt fmt-check check clippy test doc ci run clean

help:
	@printf '%s\n' \
		'make fmt        format the workspace' \
		'make check      cargo check --workspace --all-targets' \
		'make clippy     clippy with warnings as errors' \
		'make test       run all tests' \
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

ci: fmt-check clippy test doc

run:
	$(CARGO) run -p scene-cli -- $(ARGS)

clean:
	$(CARGO) clean
