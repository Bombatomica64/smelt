//! Regression tests for the source site an emitter blocker names.
//!
//! An `EmitError` is raised deep in emission — a list mutation, a tuple index,
//! a coercion — and used to carry a message about the SHAPE only. A whole-crate
//! build of Hono printed `list unshift item must match the list element type`
//! and stopped, with nothing about where; pinning one meant bisecting the
//! manifest's `exclude` list one directory at a time, which two rounds of notes
//! recorded as the cost (H48's closing section, H54's opening one).
//!
//! The function-emission entry points now attach the function's name and source
//! span on the way out (`EmitError::with_site`), so this holds for every
//! blocker raised anywhere inside a function's emission — including the ones
//! not yet written, which is the point of doing it in one place instead of at
//! each `EmitError::new`.

use super::*;
use crate::EmitError;

/// Emit the given TypeScript and return the blocker it fails with.
fn emit_error_for(ts: &str) -> EmitError {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(errors) => panic!("MIR lowering failed: {errors:?}"),
    };
    smelt_mir::opt::optimize(&mut mir);
    match crate::emit_source(&mir) {
        Ok(source) => panic!("expected an emitter blocker, got source:\n{source}"),
        Err(error) => error,
    }
}

/// A generator result crossing into `unknown` is an emitter blocker, and a
/// stable one: it is rejected in the coercion layer, several frames below the
/// function being emitted, which is exactly the shape that used to print
/// without a site.
const GENERATOR_RESULT_ERASURE: &str = r#"function* counter(): Generator<number, string, undefined> {
  yield 1;
  return "done";
}
export function probe(): unknown {
  const step = counter().next();
  return step as unknown;
}
"#;

#[test]
fn a_blocker_inside_a_function_names_the_function_and_its_span() {
    let error = emit_error_for(GENERATOR_RESULT_ERASURE);

    // The message stays about the shape; the site is a separate field so the
    // annotation cannot be applied twice or have to be parsed back out.
    assert!(
        error
            .message
            .contains("generator results require typed done/value projection"),
        "unexpected message: {}",
        error.message
    );
    let site = error.site.expect("blocker should name its site");
    assert!(site.contains("probe"), "site should name the function: {site}");
    // A unit test lowers from memory rather than a file, so the path is
    // `<memory>`; a manifest build names the real file (`smelt build` on the
    // Hono slice prints ``.../reg-exp-router/node.ts:4178..5003``). Either way
    // the byte range is the part that locates the function.
    assert!(
        site.contains(":123..185"),
        "site should name a source position: {site}"
    );
}

#[test]
fn the_rendered_blocker_reads_as_one_line() {
    // `smelt build` surfaces this through `Box<dyn Error>`, which the Rust
    // runtime prints with `Debug`; `Debug` delegates to `Display` so the one
    // line a reader sees is the readable one rather than struct syntax.
    let error = emit_error_for(GENERATOR_RESULT_ERASURE);
    let rendered = format!("{error}");
    assert!(rendered.starts_with("generator results require"), "{rendered}");
    assert!(rendered.contains("(in `probe` at "), "{rendered}");
    assert_eq!(rendered, format!("{error:?}"));
}

#[test]
fn the_innermost_site_wins() {
    // Emission nests: a method emits a closure emits a call. Whichever frame
    // annotates first is the most specific one, so a later frame must leave it
    // alone — otherwise a blocker inside a closure would be reported against
    // the enclosing method.
    let error = emit_error_for(GENERATOR_RESULT_ERASURE);
    let first = error.site.clone().expect("blocker should name its site");
    let reannotated = error.with_site(|| "`outer` at other.ts:0..1".to_owned());
    assert_eq!(reannotated.site.as_deref(), Some(first.as_str()));
}

#[test]
fn a_blocker_with_no_function_keeps_its_bare_message() {
    // Nothing outside function emission has a function to name, so the field
    // stays `None` and the rendering is unchanged. This is what keeps every
    // existing message-matching assertion in the suite honest.
    let error = EmitError::new("a shape with no function");
    assert!(error.site.is_none());
    assert_eq!(format!("{error}"), "a shape with no function");
}
