.PHONY: help build build-release build-fast check fmt fmt-check test test-fast quickwins analyze report manifest compare borrow-plan case ci lsp-report lsp-case bench bench-scanner bench-parser bench-emitter bench-pipeline bench-fixtures bench-vs-oxc

SUITE ?= compiler
BASELINE ?= js
LIMIT ?=
BENCH ?=
MANIFEST ?= .harness-manifests/$(SUITE)-$(BASELINE).json
BASE_MANIFEST ?=
CANDIDATE_MANIFEST ?=
REQUIRE_GAIN ?= 0
RUN ?=

help:
	@echo "Build targets:"
	@echo "  make build           - dev build (fast incremental, ~0.3s touch rebuild)"
	@echo "  make build-release   - production release (thin LTO, stripped, ~43s)"
	@echo "  make build-fast      - fast release (no LTO, 16 codegen units, ~25s)"
	@echo "  make check           - cargo check workspace"
	@echo ""
	@echo "Quality targets:"
	@echo "  make fmt             - cargo fmt workspace"
	@echo "  make fmt-check       - cargo fmt --check"
	@echo "  make test            - cargo test workspace"
	@echo "  make test-fast       - quick regression set"
	@echo "  make ci              - local CI command set"
	@echo ""
	@echo "LSP test targets:"
	@echo "  make lsp-report OP=quickinfo  - LSP test suite report"
	@echo "  make lsp-case CASE=name       - run one LSP test case"
	@echo "  make report                   - emitter baseline bucket report"
	@echo "  make case CASE=name           - run one emitter baseline case"
	@echo ""
	@echo "Analysis targets:"
	@echo "  make quickwins       - quick-win baseline analysis (first 500)"
	@echo "  make analyze         - failure pattern analysis (first 200)"
	@echo "  make borrow-plan     - prioritized borrowing report + reference pack"
	@echo "  make manifest        - cache-free per-case manifest (SUITE/BASELINE/MANIFEST)"
	@echo "  make compare         - compare BASE_MANIFEST and CANDIDATE_MANIFEST"
	@echo ""
	@echo "Benchmark targets:"
	@echo "  make bench           - run all benchmarks (release mode)"
	@echo "  make bench-scanner   - scanner/lexer benchmarks only"
	@echo "  make bench-parser    - parser benchmarks only"
	@echo "  make bench-emitter   - emitter benchmarks only"
	@echo "  make bench-pipeline  - full pipeline benchmarks only"
	@echo "  make bench-vs-oxc    - head-to-head parser + pipeline vs oxc"

build:
	cargo build

build-release:
	cargo build --release --bin tsc-rs

build-fast:
	cargo build --profile release-fast --bin tsc-rs

check:
	cargo check --workspace

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

test:
	cargo test --workspace

test-fast:
	cargo test -p tsc_rs_project
	cargo test -p tsc_rs_query
	cargo check -p tsc_rs_server
	cargo test -p tsc_rs_emitter --test emit_tests -- --nocapture
	cargo test -p tsc_rs_harness --test baseline_tests run_small_compiler_subset -- --nocapture

quickwins:
	cargo test -p tsc_rs_harness --test baseline_tests find_quick_wins -- --ignored --nocapture

analyze:
	cargo test -p tsc_rs_harness --test baseline_tests analyze_failure_patterns -- --ignored --nocapture

report:
	cargo run -p tsc_rs_harness --bin baseline-report -- --suite $(SUITE) $(if $(LIMIT),--limit $(LIMIT),)

manifest:
	cargo run -p tsc_rs_harness --bin baseline-report -- --suite $(SUITE) --baseline $(BASELINE) --no-cache --save-manifest $(MANIFEST)

compare:
ifndef BASE_MANIFEST
	$(error BASE_MANIFEST is required)
endif
ifndef CANDIDATE_MANIFEST
	$(error CANDIDATE_MANIFEST is required)
endif
	cargo run -p tsc_rs_harness --bin baseline-compare -- --base $(BASE_MANIFEST) --candidate $(CANDIDATE_MANIFEST) $(if $(filter 1,$(REQUIRE_GAIN)),--require-gain,) $(if $(filter 1,$(ALLOW_SKIP_RESOLUTIONS)),--allow-skip-resolutions,)

borrow-plan:
	cargo run -p tsc_rs_harness --bin borrow-plan -- --suite $(SUITE) $(if $(LIMIT),--limit $(LIMIT),) --top-topics 8 --show-cases 5 --out reference/PRIORITIZED_BORROWING_PLAN.generated.md --copy-pack reference/borrow-pack --copy-topics 3

case:
ifndef CASE
	$(error CASE is required, e.g. make case CASE=betterErrorForAccidentalCall)
endif
	@if [ "$(FULL)" = "1" ]; then \
		cargo run -p tsc_rs_harness --bin baseline-case -- $(CASE) --suite $(SUITE) --show-outputs; \
	else \
		cargo run -p tsc_rs_harness --bin baseline-case -- $(CASE) --suite $(SUITE); \
	fi

OP ?= quickinfo

lsp-report:
	cargo run -p tsc_rs_harness --bin lsp-report -- --op $(OP) $(if $(LIMIT),--limit $(LIMIT),) $(if $(KIND),--kind $(KIND),)

lsp-case:
ifndef CASE
	$(error CASE is required, e.g. make lsp-case CASE=quickInfoDisplayPartsClass)
endif
	@if [ "$(FULL)" = "1" ]; then \
		cargo run -p tsc_rs_harness --bin lsp-case -- $(CASE) --show-outputs --show-source; \
	else \
		cargo run -p tsc_rs_harness --bin lsp-case -- $(CASE) --show-outputs; \
	fi

lsp-all:
	@for op in quickinfo completions gotodefinition findallrefs signaturehelp; do \
		cargo run -p tsc_rs_harness --bin lsp-report -- --op $$op 2>&1 | grep "op="; \
	done

ci: fmt-check check test-fast

# --- Benchmarks ---

bench-fixtures:
	./crates/tsc_rs_bench/download-fixtures.sh

bench:
	cargo bench -p tsc_rs_bench $(if $(BENCH),-- $(BENCH),)

bench-scanner:
	cargo bench -p tsc_rs_bench --bench scanner $(if $(BENCH),-- $(BENCH),)

bench-parser:
	cargo bench -p tsc_rs_bench --bench parser $(if $(BENCH),-- $(BENCH),)

bench-emitter:
	cargo bench -p tsc_rs_bench --bench emitter $(if $(BENCH),-- $(BENCH),)

bench-pipeline:
	cargo bench -p tsc_rs_bench --bench pipeline $(if $(BENCH),-- $(BENCH),)

bench-vs-oxc:
	cargo bench -p tsc_rs_bench --bench compare_parser $(if $(BENCH),-- $(BENCH),)
	cargo bench -p tsc_rs_bench --bench compare_pipeline $(if $(BENCH),-- $(BENCH),)
