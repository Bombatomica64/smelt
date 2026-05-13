#![expect(
    clippy::too_many_lines,
    reason = "example CLI keeps all parsing modes in one small main function"
)]
#![expect(
    clippy::str_to_string,
    reason = "example code keeps simple string conversion close to existing style"
)]
#![expect(
    clippy::map_err_ignore,
    reason = "example maps parser diagnostics to a compact CLI error"
)]
//! Spike: parse a TypeScript file with oxc and inspect the AST.
//!
//! Usage:
//! ```bash
//! cargo run -p smelt-frontend-ts --example ts_parser -- [file] [--ast] [--comments]
//! ```

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use oxc::allocator::Allocator;
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use pico_args::Arguments;

fn main() -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    let mut args = Arguments::from_env();

    let show_ast = args.contains("--ast");
    let show_comments = args.contains("--comments");
    let name = args
        .free_from_str()
        .unwrap_or_else(|_| "test.ts".to_string());

    let path = Path::new(&name);
    let source_text = fs::read_to_string(path).map_err(|_| format!("Missing '{name}'"))?;
    let source_type = SourceType::from_path(path).map_err(|error| error.to_string())?;

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source_text, source_type)
        .with_options(ParseOptions {
            parse_regular_expression: true,
            ..ParseOptions::default()
        })
        .parse();

    if !ret.errors.is_empty() {
        for parse_error in ret.errors {
            let rendered = parse_error.with_source_code(source_text.clone());
            writeln!(stdout, "{rendered}").map_err(|error| error.to_string())?;
        }
        writeln!(stdout, "Parsed with errors.").map_err(|error| error.to_string())?;
        return Ok(());
    }

    // Run semantic analysis — populates node_id, scope_id, symbol_id, reference_id
    let semantic_ret = SemanticBuilder::new().build(&ret.program);

    if !semantic_ret.errors.is_empty() {
        for semantic_error in semantic_ret.errors {
            let rendered = semantic_error.with_source_code(source_text.clone());
            writeln!(stdout, "{rendered}").map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    let semantic = semantic_ret.semantic;

    if show_comments {
        writeln!(stdout, "Comments:").map_err(|error| error.to_string())?;
        for comment in &ret.program.comments {
            let s = comment.content_span().source_text(&source_text);
            writeln!(stdout, "{s}").map_err(|error| error.to_string())?;
        }
    }

    if show_ast {
        writeln!(stdout, "AST:").map_err(|error| error.to_string())?;
        writeln!(stdout, "AST output omitted in this example build")
            .map_err(|error| error.to_string())?;
    }

    // Always print a summary of what the semantic pass found
    let scoping = semantic.scoping();

    writeln!(stdout, "Symbols ({}):", scoping.symbols_len()).map_err(|error| error.to_string())?;
    for id in scoping.symbol_ids() {
        let symbol_name = scoping.symbol_name(id);
        let span = scoping.symbol_span(id);
        writeln!(
            stdout,
            "  symbol `{symbol_name}` at {}..{}",
            span.start, span.end
        )
        .map_err(|error| error.to_string())?;
    }

    writeln!(stdout, "References (per symbol):").map_err(|error| error.to_string())?;
    for id in scoping.symbol_ids() {
        let symbol_name = scoping.symbol_name(id);
        for ref_id in scoping.get_resolved_reference_ids(id) {
            let reference = scoping.get_reference(*ref_id);
            let span = semantic.reference_span(reference);
            writeln!(
                stdout,
                "  ref for `{symbol_name}` at {}..{}",
                span.start, span.end
            )
            .map_err(|error| error.to_string())?;
        }
    }

    writeln!(stdout, "Parsed successfully.").map_err(|error| error.to_string())?;

    Ok(())
}
