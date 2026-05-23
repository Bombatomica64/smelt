---
type: "query"
date: "2026-05-22T18:47:08.836632+00:00"
question: "Why does FileId connect lower.rs to major lowering, codegen, manifest, and workspace nodes?"
contributor: "graphify"
source_nodes: ["FileId", "lower_combined", "scan_python_imports", "lower_typescript_files", "lower_python_files", "FunctionEmitter", "SmeltWorkspace", "lower_hir"]
---

# Q: Why does FileId connect lower.rs to major lowering, codegen, manifest, and workspace nodes?

## Answer

FileId is the shared HIR source identity type, defined in smelt-hir/src/ids.rs:L7, and the graph shows it crossing the CLI lowering pipeline, manifest/import scanning, GUI compilation, codegen emitters, frontend test helpers, and MIR tests/lowering. The direct FileId neighbors include lower_typescript_files at smelt-cli/src/lowering.rs:L112 via L126, lower_python_files at L140 via L154, scan_python_imports at smelt-cli/src/manifest.rs:L661 via L662, lower_combined at smelt-gui/src/compiler.rs:L126 via L136, FunctionEmitter construction in smelt-codegen-rust/src/emitter/core.rs:L45, frontend test helpers, and MIR tests. The path into workspace goes FileId -> lower_combined -> compile -> run_compile -> workspace.rs -> SmeltWorkspace. The path into lower.rs goes FileId -> MIR tests -> lower_hir -> lower.rs. Most FileId edges are marked INFERRED, so treat this as a dependency map from shared identifier usage rather than proof of runtime calls.

## Source Nodes

- FileId
- lower_combined
- scan_python_imports
- lower_typescript_files
- lower_python_files
- FunctionEmitter
- SmeltWorkspace
- lower_hir