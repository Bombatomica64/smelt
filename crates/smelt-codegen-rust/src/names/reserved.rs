//! The reserved set: every name a source spelling may not render to.
//!
//! One list per reason, each with the evidence that requires it. The tests in
//! `tests/reserved_name_tests.rs` derive the required names from the emitted
//! runtime, the committed example crates and the emitter source, and fail when
//! one is missing here.

use super::NameSpace;

/// Whether `name` carries one of the emitter's reserved prefixes.
///
/// After any leading underscores, a name beginning `smelt`, `Smelt` or `SMELT`
/// followed by `_`, an upper-case letter, a digit or nothing belongs to the
/// generated crate itself: the runtime's helpers and types (`smelt_typeof`,
/// `SmeltList`, every `smelt_stdlib::runtime_symbols` constant), the
/// emitter's per-program items (`SmeltUnion3`, `__smelt_init_Base`,
/// `SMELT_GLOBAL_X_0`), its template temporaries (`smelt_needle`,
/// `_smelt_tmp_4`), and the line-classifier of the SmeltUnknown report, which
/// reads `fn smelt_` as runtime prelude. A source name never takes one, so
/// none of those can be shadowed and the report cannot mistake user code for
/// runtime. `smelter` is not reserved: only the word `smelt` itself is.
pub(crate) fn has_emitter_prefix(name: &str) -> bool {
    let rest = name.trim_start_matches('_');
    ["smelt", "Smelt", "SMELT"].iter().any(|prefix| {
        rest.strip_prefix(prefix).is_some_and(|tail| {
            tail.chars().next().is_none_or(|next| {
                next == '_' || next.is_ascii_uppercase() || next.is_ascii_digit()
            })
        })
    })
}

/// Whether the escaped `name` is reserved in `space` and must take a `_`.
pub(super) fn is_reserved(name: &str, space: NameSpace) -> bool {
    name.is_empty()
        || name == "_"
        || is_rust_keyword(name)
        || is_rust_prelude_name(name)
        || (space == NameSpace::Members && is_emitter_member_name(name))
}

/// Returns true when the identifier names an item of Rust's prelude that the
/// generated runtime spells unqualified.
///
/// A source class named `Box` or `Vec` is legal TypeScript, but declaring
/// `struct Box` shadows `std::boxed::Box` for the whole crate, and every
/// prelude line that writes `Box<dyn ..>` then refers to the user's struct
/// (E0107). Renaming the source item the way a keyword is renamed keeps both:
/// the declaration and every reference go through this module. `JsString`
/// and `JsStr` are the runtime's JavaScript string types, which every
/// TypeScript crate spells unqualified in the same way.
///
/// `main` is the same case one level up: the crate root defines the binary's
/// entry point `fn main`, so a source function named `main` (`async function
/// main() {..}; main()`, a common script shape) was a second `main` (E0428).
///
/// The list is every Rust prelude item the runtime or the emitter spells
/// unqualified (closure types `dyn Fn(..)`, `impl Clone for`, derives,
/// `IntoIterator` bounds) and every runtime item declared at the crate root
/// without a `smelt` prefix (`JsString`, `DateLike`, `IntoSmeltUnknown`): a
/// source `type Fn` declared as `struct Fn` hid the `Fn` trait from every
/// closure type of the crate (E0404). `tests/reserved_name_tests.rs` derives
/// the required set from the rendered runtime and the committed example crates
/// and fails when a name is missing here.
pub(crate) fn is_rust_prelude_name(name: &str) -> bool {
    matches!(
        name,
        "Box"
            | "Vec"
            | "Option"
            | "Result"
            | "String"
            | "Some"
            | "None"
            | "Ok"
            | "Err"
            | "main"
            | "JsString"
            | "JsStr"
            | "AsRef"
            | "Clone"
            | "Copy"
            | "Default"
            | "Drop"
            | "Eq"
            | "Ord"
            | "PartialEq"
            | "PartialOrd"
            | "Fn"
            | "FnMut"
            | "FnOnce"
            | "From"
            | "Into"
            | "FromIterator"
            | "IntoIterator"
            | "Iterator"
            | "IntoFuture"
            | "Send"
            | "Sized"
            | "ToOwned"
            | "ToString"
            | "drop"
            | "DateLike"
            | "IntoSmeltUnknown"
    )
}

/// Associated items the emitter itself puts on, or calls through, a user type.
///
/// An inherent method beats a trait method in Rust's method resolution, so a
/// source method named `clone` turned every `receiver.clone()` the emitter
/// writes to copy a handle into a call of the user's method (a class whose
/// `clone()` builds a new instance made every internal copy a fresh object).
/// The names are the methods of every trait the emitter derives or implements
/// for a generated type (see [`crate::derive_impls::EMITTED_TRAITS`])
/// and the constructor `new` every class `impl` declares.
fn is_emitter_member_name(name: &str) -> bool {
    name == "new" || crate::derive_impls::is_emitted_trait_method(name)
}

/// Returns true when the identifier is reserved by Rust.
///
/// This is the full keyword list of the Rust 2024 edition generated crates
/// are built with: the strict keywords, the 2018 additions (`async`, `await`,
/// `dyn`), and every reserved keyword, including `gen`, which the 2024 edition
/// reserved for generator blocks. A name on it is renamed with a trailing `_`
/// (not spelled as a raw identifier) because `self`, `Self`, `super` and
/// `crate` cannot be raw identifiers at all, and one scheme for every keyword
/// keeps a declaration and its references agreeing without knowing which
/// edition introduced the word. The weak keywords (`union`, `raw`, `safe`,
/// `macro_rules`) remain valid identifiers and are deliberately absent.
pub(crate) fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "gen"
    )
}
