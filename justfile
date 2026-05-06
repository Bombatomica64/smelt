smelt := "RUSTFLAGS='-Awarnings' cargo run -q -p smelt-cli --"
module_demo := ".tmp_module_demo"

# Run a test for a given directory (e.g. examples/typescript/end-to-end/01_number)
test dir:
	@echo "Testing {{dir}}..."
	@echo "# Test Output for {{dir}}" > output.md
	@echo "## MIR" >> output.md
	@echo '```text' >> output.md
	@if [ -f {{dir}}/input.ts ]; then {{smelt}} dump-mir {{dir}}/input.ts >> output.md; else {{smelt}} dump-mir {{dir}}/input.py >> output.md; fi
	@echo '```' >> output.md
	@echo "" >> output.md
	@echo "## Generated Rust Code" >> output.md
	@echo '```rust' >> output.md
	@rm -rf .tmp_test && mkdir -p .tmp_test/src
	@if [ -f {{dir}}/input.ts ]; then cp {{dir}}/input.ts .tmp_test/src/main.ts && entry='src/main.ts'; else cp {{dir}}/input.py .tmp_test/src/main.py && entry='src/main.py'; fi; printf '%s\n' '[project]' 'name = "test-app"' 'version = "0.1.0"' '' '[sources]' "entries = [\"$entry\"]" '' '[output]' 'target = "./dist"' 'crate-name = "test_app"' 'build = true' '' '[runtime]' 'clone-strategy = "aggressive"' > .tmp_test/Smelt.toml
	@{{smelt}} --manifest-path .tmp_test/Smelt.toml build
	@cat .tmp_test/dist/src/main.rs >> output.md
	@echo '```' >> output.md
	@echo "" >> output.md
	@echo "## Execution Output" >> output.md
	@echo '```text' >> output.md
	@cargo run -q --manifest-path .tmp_test/dist/Cargo.toml >> output.md 2>&1 || true
	@echo '```' >> output.md
	@rm -rf .tmp_test
	@echo "Done! Check output.md"

# Format everything
fmt:
	cargo fmt --all

# Lint everything
lint:
	cargo clippy --all-targets

# Run tests
test-all:
	cargo test

# Run the focused module-linking and stub-generation tests
test-modules:
	cargo test -p smelt-cli --test hir_cli check_emits_typescript_declaration_stubs_for_linked_modules
	cargo test -p smelt-cli --test hir_cli check_emits_python_stubs_for_linked_modules

# Try mixed TypeScript -> Python module linking, build the generated crate, and run it
try-modules:
	@rm -rf {{module_demo}}
	@mkdir -p {{module_demo}}/src
	@printf '%s\n' '[project]' 'name = "cross-run"' 'version = "0.1.0"' '' '[sources]' 'entries = ["src/math.ts", "src/main.py"]' '' '[output]' 'target = "./dist"' 'crate-name = "cross_run"' 'build = true' '' '[runtime]' 'clone-strategy = "aggressive"' > {{module_demo}}/Smelt.toml
	@printf '%s\n' 'export function add(a: number, b: number): number {' '  return a + b;' '}' > {{module_demo}}/src/math.ts
	@printf '%s\n' 'from math import add' 'result: float = add(2.0, 3.0)' 'print(result)' > {{module_demo}}/src/main.py
	@{{smelt}} --manifest-path {{module_demo}}/Smelt.toml build
	@echo "== Generated stubs =="
	@find {{module_demo}}/src \( -name '*.d.ts' -o -name '*.pyi' \) -maxdepth 1 -print -exec sed -n '1,80p' {} \;
	@echo "== Generated Rust output =="
	@cargo run -q --manifest-path {{module_demo}}/dist/Cargo.toml
	@echo "Demo left in {{module_demo}}"

# Dump HIR for the current async TypeScript examples.
try-async-hir:
	@echo "== Async function example =="
	@{{smelt}} dump-hir examples/typescript/hir/09_async_function.ts
	@echo ""
	@echo "== Async class method example =="
	@{{smelt}} dump-hir examples/typescript/hir/10_async_class_method.ts

# Remove generated module-linking demo files
clean-modules:
	rm -rf {{module_demo}}
