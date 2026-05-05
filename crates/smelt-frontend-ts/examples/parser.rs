#![expect(clippy::print_stdout, reason = "example prints parsed AST output")]
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
#![expect(
    clippy::unwrap_in_result,
    reason = "example unwraps parser program only after checking parser errors"
)]
//! Spike: parse a TypeScript file with oxc and inspect the AST.
//!
//! Usage:
//! ```bash
//! cargo run -p smelt-frontend-ts --example parser [file] [--ast] [--comments]
//! ```

use std::{fs, path::Path};

use oxc::allocator::Allocator;
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use pico_args::Arguments;

fn main() -> Result<(), String> {
    let mut args = Arguments::from_env();

    let show_ast = args.contains("--ast");
    let show_comments = args.contains("--comments");
    let name = args
        .free_from_str()
        .unwrap_or_else(|_| "test.ts".to_string());

    let path = Path::new(&name);
    let source_text = fs::read_to_string(path).map_err(|_| format!("Missing '{name}'"))?;
    let source_type = SourceType::from_path(path).unwrap();

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source_text, source_type)
        .with_options(ParseOptions {
            parse_regular_expression: true,
            ..ParseOptions::default()
        })
        .parse();

    if !ret.errors.is_empty() {
        for error in ret.errors {
            let error = error.with_source_code(source_text.clone());
            println!("{error:?}");
        }
        println!("Parsed with errors.");
        return Ok(());
    }

    // Run semantic analysis — populates node_id, scope_id, symbol_id, reference_id
    let semantic_ret = SemanticBuilder::new().build(&ret.program);

    if !semantic_ret.errors.is_empty() {
        for error in semantic_ret.errors {
            let error = error.with_source_code(source_text.clone());
            println!("{error:?}");
        }
        return Ok(());
    }

    let semantic = semantic_ret.semantic;

    if show_comments {
        println!("Comments:");
        for comment in &ret.program.comments {
            let s = comment.content_span().source_text(&source_text);
            println!("{s}");
        }
    }

    if show_ast {
        println!("AST:");
        println!("{:#?}", ret.program);
    }

    // Always print a summary of what the semantic pass found
    let scoping = semantic.scoping();

    println!("Symbols ({}):", scoping.symbols_len());
    for id in scoping.symbol_ids() {
        let name = scoping.symbol_name(id);
        let span = scoping.symbol_span(id);
        println!("  {id:?} `{name}` at {span:?}");
    }

    println!("References (per symbol):");
    for id in scoping.symbol_ids() {
        let name = scoping.symbol_name(id);
        for ref_id in scoping.get_resolved_reference_ids(id) {
            let reference = scoping.get_reference(*ref_id);
            let span = semantic.reference_span(reference);
            println!("  {ref_id:?} `{name}` at {span:?}");
        }
    }

    println!("Parsed successfully.");

    Ok(())
}
