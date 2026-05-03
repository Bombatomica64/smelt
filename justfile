smelt := "RUSTFLAGS='-Awarnings' cargo run -q -p smelt-cli --"

# Run a test for a given directory (e.g. examples/typescript/end-to-end/01_number)
test dir:
	@echo "Testing {{dir}}..."
	@echo "# Test Output for {{dir}}" > output.md
	@echo "## MIR" >> output.md
	@echo '```text' >> output.md
	@{{smelt}} dump-mir {{dir}}/input.ts >> output.md || {{smelt}} dump-mir {{dir}}/input.py >> output.md
	@echo '```' >> output.md
	@echo "" >> output.md
	@echo "## Generated Rust Code" >> output.md
	@echo '```rust' >> output.md
	@rm -rf .tmp_test && mkdir -p .tmp_test/src
	@if [ -f {{dir}}/input.ts ]; then cp {{dir}}/input.ts .tmp_test/src/main.ts; else cp {{dir}}/input.py .tmp_test/src/main.py; fi
	@echo '[project]\nname = "test-app"\nversion = "0.1.0"\n\n[sources]\nentries = ["src/main.ts", "src/main.py"]\n\n[output]\ntarget = "./dist"\ncrate-name = "test_app"\nbuild = true\n\n[runtime]\nclone-strategy = "aggressive"' > .tmp_test/Smelt.toml
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
