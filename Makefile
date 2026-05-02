SMELT := cargo run -p smelt-cli --

# ─── TypeScript examples (full build via Smelt.toml) ──────────────────────────

.PHONY: ts-14
ts-14: ## TS: c-style for loop → Rust
	$(SMELT) --manifest-path examples/typescript/end-to-end/14_c_for_loop/Smelt.toml build

.PHONY: ts-14-hir
ts-14-hir: ## TS: dump HIR for c-style for loop
	$(SMELT) --manifest-path examples/typescript/end-to-end/14_c_for_loop/Smelt.toml build --hir

.PHONY: all-ts
all-ts: ts-14 ## Run all TypeScript examples

# ─── Python examples (dump-mir; full build pending Python pipeline) ────────────

.PHONY: py-01
py-01: ## PY: number variable
	$(SMELT) dump-mir examples/python/end-to-end/01_number/input.py

.PHONY: py-02
py-02: ## PY: string variable
	$(SMELT) dump-mir examples/python/end-to-end/02_string/input.py

.PHONY: py-03
py-03: ## PY: boolean + logical ops
	$(SMELT) dump-mir examples/python/end-to-end/03_boolean/input.py

.PHONY: py-04
py-04: ## PY: None value
	$(SMELT) dump-mir examples/python/end-to-end/04_none/input.py

.PHONY: py-05
py-05: ## PY: while loop sum
	$(SMELT) dump-mir examples/python/end-to-end/05_while_sum/input.py

.PHONY: py-06
py-06: ## PY: function definition + call
	$(SMELT) dump-mir examples/python/end-to-end/06_function/input.py

.PHONY: py-07
py-07: ## PY: if / elif / else
	$(SMELT) dump-mir examples/python/end-to-end/07_if_else/input.py

.PHONY: py-08
py-08: ## PY: match statement
	$(SMELT) dump-mir examples/python/end-to-end/08_match/input.py

.PHONY: all-py
all-py: py-01 py-02 py-03 py-04 py-05 py-06 py-07 py-08 ## Run all Python examples

# ─── HIR variants for Python ──────────────────────────────────────────────────

.PHONY: py-01-hir
py-01-hir: ## PY: dump HIR for number
	$(SMELT) dump-hir examples/python/end-to-end/01_number/input.py

.PHONY: py-05-hir
py-05-hir: ## PY: dump HIR for while loop
	$(SMELT) dump-hir examples/python/end-to-end/05_while_sum/input.py

.PHONY: py-06-hir
py-06-hir: ## PY: dump HIR for function
	$(SMELT) dump-hir examples/python/end-to-end/06_function/input.py

# ─── Convenience ──────────────────────────────────────────────────────────────

.PHONY: all
all: all-ts all-py ## Run every example (TS build + Python dump-mir)

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) \
	  | awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
