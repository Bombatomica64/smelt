---
type: "path_query"
date: "2026-05-22T18:50:01.656785+00:00"
question: "Path from lower_combined to FunctionEmitter"
contributor: "graphify"
source_nodes: ["lower_combined", "FunctionEmitter", "FileId"]
---

# Q: Path from lower_combined to FunctionEmitter

## Answer

The shortest path to the central codegen emitter is lower_combined() at smelt-gui/src/compiler.rs:L126 -> FileId at smelt-hir/src/ids.rs:L7 -> FunctionEmitter::new() at smelt-codegen-rust/src/emitter/core.rs:L9 -> FunctionEmitter<'mir> at smelt-codegen-rust/src/emitter/core.rs:L7. The lower_combined to FileId and FileId to FunctionEmitter::new edges are INFERRED, while FunctionEmitter::new being a method on FunctionEmitter<'mir> is EXTRACTED. A longer path to emitter/unknown.rs goes through FunctionEmitter::new(), local_names(), sanitize_ident(), and class_unknown_object_text(). This means the graph connects GUI lowering to codegen primarily through shared HIR source identity/provenance rather than a direct compile-time call chain.

## Source Nodes

- lower_combined
- FunctionEmitter
- FileId