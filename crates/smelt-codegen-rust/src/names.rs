//! Rendered Rust names: the one place a source name becomes a Rust identifier.
//!
//! A [`Symbol`](smelt_hir::Symbol) is identity; the Rust identifier it renders
//! to is not. Two source names can render alike (`fooBar` and `foo_bar` both
//! fold to `foo_bar`, `Box` escapes to `Box_` which a source `Box_` already
//! spells), and a source name can render to a name the generated crate already
//! uses (a class `SmeltList`, a method `clone`, a local `smelt_needle`). Rust
//! only cares about the final string, per namespace, so uniqueness is decided
//! there and nowhere else:
//!
//! * [`RustName`] is a rendered identifier. Its field is private: the only way
//!   to obtain one is to claim it from a [`NameScope`], which escapes the
//!   spelling against the scope's reserved set ([`reserved`]) and de-duplicates
//!   it against every name already claimed in that scope.
//! * [`RustNames`] is the crate's plan, built once per emission: every declared
//!   type, free function and method has its name decided up front, keyed by
//!   its stable identity (`Symbol`, `FuncId`). Declarations and references both
//!   read the plan, so they cannot disagree.
//! * Members that are not crate items (struct fields, generic parameters)
//!   render through [`RustIdent`], which only this module constructs: the
//!   raw sanitizer is private, so no other module can turn a string into an
//!   identifier without passing the escape rules.
//!
//! The reserved set is owned here (see [`reserved`]): Rust keywords, the
//! prelude and runtime crate-root names, `_`, the emitter's reserved prefixes
//! (`smelt_`, `Smelt`, `__smelt`, `SMELT_`, which also cover every
//! `smelt_stdlib::runtime_symbols` helper and every emitter template
//! temporary), and the trait methods the emitter calls on user types.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter shards"
)]

mod plan;
pub(crate) mod reserved;

use std::collections::HashSet;

pub(crate) use plan::{RustNames, TypeDecl, type_claim_order};

/// The Rust namespace a name is claimed in, which decides its reserved set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameSpace {
    /// Crate-root types and traits (structs, enums, generated traits).
    Types,
    /// Crate-root values: free functions (statics carry an emitter prefix).
    Values,
    /// Associated items of one `impl` block: methods, statics, constructors.
    Members,
    /// The locals and parameters of one function body.
    Locals,
    /// The fields of one struct.
    Fields,
    /// Generic type parameters of one item.
    TypeParams,
    /// Generated module files: compared case-insensitively, since a
    /// case-insensitive filesystem cannot hold `Foo.rs` beside `foo.rs`.
    Modules,
}

impl NameSpace {
    /// Whether a SOURCE name in this namespace may not carry an emitter prefix.
    ///
    /// Item, member and local names share the generated crate with the
    /// runtime's `smelt_*`/`Smelt*` items and the emitter's `smelt_*`
    /// template temporaries, and a struct's fields share it with the fields
    /// the emitter adds (`_smelt_phantom`) and the slots lowerings mint
    /// (`__smelt_call`). Generic parameters and module files live in a
    /// namespace of their own.
    const fn reserves_emitter_prefixes(self) -> bool {
        matches!(
            self,
            Self::Types | Self::Values | Self::Members | Self::Locals | Self::Fields
        )
    }
}

/// A name to claim, with who wrote it.
///
/// The emitter's prefixes (`smelt_`, `Smelt`, `__smelt`, `SMELT_`) are the
/// generated crate's own namespace. A name a lowering minted lives there
/// (`smelt_global_init__x__0`, `__smelt_spread`) and keeps its spelling; a
/// name the program spelled is moved out of it (`smelt_typeof` ->
/// `src_smelt_typeof`), so the two can never meet. Who wrote a name is the
/// symbol's [`SymbolOrigin`](smelt_hir::SymbolOrigin), never its spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spelling<'a> {
    /// A name the source program spells.
    Source(&'a str),
    /// A name a lowering minted ([`smelt_hir::SynthesizedName`]).
    Synthesized(&'a str),
}

impl<'a> Spelling<'a> {
    /// The spelling of `symbol`, tagged with the symbol's origin.
    pub(crate) fn of(mir: &'a smelt_mir::Mir, symbol: smelt_hir::Symbol) -> Option<Self> {
        let text = mir.symbols.get(symbol)?;
        Some(match mir.symbols.origin(symbol) {
            smelt_hir::SymbolOrigin::Source => Self::Source(text),
            smelt_hir::SymbolOrigin::Synthesized => Self::Synthesized(text),
        })
    }

    /// The spelled text.
    pub(crate) const fn text(self) -> &'a str {
        match self {
            Self::Source(text) | Self::Synthesized(text) => text,
        }
    }
}

/// A Rust identifier claimed from a [`NameScope`].
///
/// Only this module constructs one, and only by claiming it: holding a
/// `RustName` proves the text is a valid identifier that avoids its scope's
/// reserved set and every other name claimed in that scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct RustName(String);

impl RustName {
    /// The rendered identifier text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RustName {
    /// Writes the rendered identifier text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A name could not be claimed exactly as spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameClash {
    /// The rendered text that was already taken or reserved.
    pub(crate) name: String,
}

impl std::fmt::Display for NameClash {
    /// Describes the clash for an `EmitError`.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the Rust name `{}` is already taken in its scope",
            self.name
        )
    }
}

/// One Rust scope's claimed names.
///
/// A scope is one namespace of one Rust item: the crate's types, the crate's
/// values, one `impl` block, one function body, the module files of a crate.
/// Claims are first come, first served; a later spelling that renders like an
/// earlier one gets a numeric suffix rather than shadowing it.
#[derive(Debug, Clone)]
pub(crate) struct NameScope {
    /// Which reserved set applies.
    space: NameSpace,
    /// Every rendered name claimed (lower-cased for [`NameSpace::Modules`]).
    taken: HashSet<String>,
}

impl NameScope {
    /// An empty scope of `space`.
    pub(crate) fn new(space: NameSpace) -> Self {
        Self {
            space,
            taken: HashSet::new(),
        }
    }

    /// The comparison key of `text` in this scope.
    fn key(&self, text: &str) -> String {
        if self.space == NameSpace::Modules {
            text.to_ascii_lowercase()
        } else {
            text.to_owned()
        }
    }

    /// Whether `text` is already claimed or held in this scope.
    pub(crate) fn is_taken(&self, text: &str) -> bool {
        self.taken.contains(&self.key(text))
    }

    /// Holds a name the emitter itself spells in this scope (a constructor's
    /// `new`, an entry point's `main`, a crate function a body may call), so
    /// no claim can take it.
    pub(crate) fn hold(&mut self, emitter_name: &str) {
        let key = self.key(emitter_name);
        self.taken.insert(key);
    }

    /// Claims a name the emitter itself spells (the binary's `fn main`),
    /// bypassing the escape: it must still be free in this scope.
    pub(crate) fn claim_emitter_name(&mut self, text: &'static str) -> Result<RustName, NameClash> {
        if !self.taken.insert(self.key(text)) {
            return Err(NameClash {
                name: text.to_owned(),
            });
        }
        Ok(RustName(text.to_owned()))
    }

    /// Claims a name the emitter mints itself (`_smelt_tmp_4`, `arg_0`): not a
    /// source spelling, so not escaped, but still renamed apart from every
    /// name already claimed.
    pub(crate) fn claim_minted(&mut self, text: &str) -> RustName {
        self.claim_free(text.to_owned(), None)
    }

    /// Claims the rendering of the source spelling `spelling`: escaped against
    /// the reserved set, then `_1`, `_2`, … until it is free.
    pub(crate) fn claim(&mut self, spelling: Spelling<'_>) -> RustName {
        let base = escape(spelling, self.space);
        self.claim_free(base, None)
    }

    /// Like [`Self::claim`], but a taken base tries `<base>_<salt>` first.
    ///
    /// The salt is a stable id of the claimant (a `FuncId`), so a function
    /// renamed apart keeps its name when an unrelated declaration is added.
    pub(crate) fn claim_salted(&mut self, spelling: Spelling<'_>, salt: u32) -> RustName {
        let base = escape(spelling, self.space);
        self.claim_free(base, Some(salt))
    }

    /// Claims `<spelling>_<salt>` (escaped), as a declaration that must not keep
    /// its bare spelling does: the duplicated free functions of a crate.
    pub(crate) fn claim_suffixed(&mut self, spelling: Spelling<'_>, salt: u32) -> RustName {
        let base = format!("{}_{salt}", escape(spelling, self.space));
        self.claim_free(base, None)
    }

    /// Claims the first free candidate of `base`, `<base>_<salt>`, then
    /// numbered suffixes.
    fn claim_free(&mut self, base: String, salt: Option<u32>) -> RustName {
        if self.taken.insert(self.key(&base)) {
            return RustName(base);
        }
        if let Some(id) = salt {
            let salted = format!("{base}_{id}");
            if self.taken.insert(self.key(&salted)) {
                return RustName(salted);
            }
        }
        let mut suffix = 1_usize;
        loop {
            let candidate = format!("{base}_{suffix}");
            if self.taken.insert(self.key(&candidate)) {
                return RustName(candidate);
            }
            suffix = suffix.saturating_add(1);
        }
    }
}

/// A valid Rust identifier for a member that is not a crate item: a struct
/// field, a generic parameter, a host method spelled by a runtime type.
///
/// Only this module constructs one, through the field and parameter escapes
/// below, so every member spelling passes the same keyword and prelude rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RustIdent(String);

impl RustIdent {
    /// Returns the rendered identifier text.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the identifier and returns the rendered source text.
    #[must_use]
    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for RustIdent {
    /// Writes the rendered identifier text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The field identifier `spelling` renders to with no other field to rename
/// it apart from: the escape alone. [`RustNames::field`] is the planned name
/// and answers this for a symbol no generated struct declares.
pub(crate) fn lone_field(spelling: Spelling<'_>) -> RustIdent {
    RustIdent(escape(spelling, NameSpace::Fields))
}

/// The name of a generated trait's method for the source member `spelling`:
/// escaped as an associated item, so it never shadows a trait method the
/// emitter calls on the implementing type.
pub(crate) fn trait_member(spelling: Spelling<'_>) -> RustIdent {
    RustIdent(escape(spelling, NameSpace::Members))
}

/// The identifier of a generic type parameter spelled `spelling`.
pub(crate) fn type_param(spelling: &str) -> RustIdent {
    RustIdent(escape(Spelling::Source(spelling), NameSpace::TypeParams))
}

/// The identifier of a method a runtime or host type defines, spelled by its
/// source name: escaped against keywords only, never renamed apart, since the
/// runtime's own spelling is what it must match.
pub(crate) fn host_member(spelling: &str) -> RustIdent {
    // A runtime type's own method: its spelling is the runtime's, which the
    // prefix rule must not move (`smelt_*` methods of runtime types).
    RustIdent(escape(Spelling::Synthesized(spelling), NameSpace::Fields))
}

/// A readable fragment of a source name, for an emitter-owned name that
/// embeds it after an emitter prefix (`SMELT_GLOBAL_<fragment>_<index>`).
///
/// The fragment is never an identifier on its own: the emitter's prefix and
/// unique suffix are what make the composed name collision-free.
pub(crate) fn fragment(spelling: &str) -> String {
    sanitize(spelling)
}

/// The rendering `spelling` would claim in an empty scope of `space`, for
/// comparing two spellings before either is claimed.
fn render(spelling: Spelling<'_>, space: NameSpace) -> String {
    escape(spelling, space)
}

/// Escapes a spelling into a Rust identifier of `space`.
///
/// Characters Rust does not allow become `_`, a leading digit becomes `_`; a
/// SOURCE name carrying an emitter prefix is moved out of it (`src_`/`Src`),
/// while a synthesized one keeps it; then a keyword, a prelude or runtime
/// crate-root name, `_` and (for members) a trait method the emitter calls
/// take a trailing `_` until free.
fn escape(spelling: Spelling<'_>, space: NameSpace) -> String {
    let mut out = sanitize(spelling.text());
    if matches!(spelling, Spelling::Source(_))
        && space.reserves_emitter_prefixes()
        && reserved::has_emitter_prefix(&out)
    {
        // The leading underscores go: `src_` already marks the name, and a
        // leading `_` would read as "unused" to a Rust reader.
        let stem = out.trim_start_matches('_');
        out = if space == NameSpace::Types {
            format!("Src{stem}")
        } else {
            format!("src_{stem}")
        };
    }
    while reserved::is_reserved(&out, space) {
        out.push('_');
    }
    out
}

/// Replaces every character Rust does not allow in an identifier with `_`.
fn sanitize(spelling: &str) -> String {
    let mut out = String::with_capacity(spelling.len());
    for (idx, ch) in spelling.chars().enumerate() {
        if ch == '_' || ch.is_ascii_alphanumeric() && (idx > 0 || !ch.is_ascii_digit()) {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{NameScope, NameSpace, Spelling, escape, lone_field};

    /// A source spelling.
    const fn src(text: &str) -> Spelling<'_> {
        Spelling::Source(text)
    }

    /// Two source spellings that render alike get two names.
    #[test]
    fn a_scope_renames_apart_two_spellings_that_render_alike() {
        let mut scope = NameScope::new(NameSpace::Types);
        let boxed = scope.claim(src("Box"));
        let user = scope.claim(src("Box_"));
        assert_eq!(boxed.as_str(), "Box_");
        assert_eq!(user.as_str(), "Box__1");
    }

    /// `$` and `_` sanitize to the wildcard; both escape and stay distinct.
    #[test]
    fn the_wildcard_never_renders_and_two_wildcards_stay_apart() {
        let mut scope = NameScope::new(NameSpace::Values);
        let dollar = scope.claim(src("$"));
        let underscore = scope.claim(src("_"));
        assert_eq!(dollar.as_str(), "__");
        assert_eq!(underscore.as_str(), "___1");
    }

    /// The emitter's prefixes never come out of a source name, while a name a
    /// lowering minted keeps them.
    #[test]
    fn emitter_prefixes_are_moved_out_of_source_names_only() {
        assert_eq!(escape(src("SmeltList"), NameSpace::Types), "SrcSmeltList");
        assert_eq!(
            escape(src("smelt_typeof"), NameSpace::Values),
            "src_smelt_typeof"
        );
        assert_eq!(escape(src("__smelt_x"), NameSpace::Locals), "src_smelt_x");
        assert_eq!(
            escape(src("SMELT_GLOBAL"), NameSpace::Members),
            "src_SMELT_GLOBAL"
        );
        assert_eq!(
            escape(src("_smelt_phantom"), NameSpace::Fields),
            "src_smelt_phantom"
        );
        assert_eq!(escape(src("smelter"), NameSpace::Values), "smelter");
        let minted = Spelling::Synthesized("smelt_global_init__x__0");
        assert_eq!(escape(minted, NameSpace::Values), "smelt_global_init__x__0");
        assert_eq!(
            escape(Spelling::Synthesized("__smelt_call"), NameSpace::Fields),
            "__smelt_call"
        );
    }

    /// A method never takes the name of a trait method the emitter calls.
    #[test]
    fn members_avoid_trait_method_names() {
        assert_eq!(escape(src("clone"), NameSpace::Members), "clone_");
        assert_eq!(escape(src("new"), NameSpace::Members), "new_");
        assert_eq!(escape(src("clone"), NameSpace::Fields), "clone");
        assert_eq!(escape(src("clone"), NameSpace::Values), "clone");
    }

    /// Module files compare case-insensitively.
    #[test]
    fn module_names_collide_case_insensitively() {
        let mut scope = NameScope::new(NameSpace::Modules);
        assert_eq!(scope.claim(src("Foo")).as_str(), "Foo");
        assert_eq!(scope.claim(src("foo")).as_str(), "foo_1");
        assert_eq!(scope.claim(src("foo-bar")).as_str(), "foo_bar");
        assert_eq!(scope.claim(src("foo_bar")).as_str(), "foo_bar_1");
    }

    /// Invalid characters, leading digits and every keyword of the 2024
    /// edition are escaped; weak keywords stay ordinary identifiers.
    #[test]
    fn sanitizes_invalid_and_reserved_identifiers() {
        let field = |text: &str| lone_field(src(text)).into_string();
        assert_eq!(field("valid_name"), "valid_name");
        assert_eq!(field("1st-value"), "_st_value");
        assert_eq!(field("fn"), "fn_");
        // Reserved by the 2024 edition only; `fn gen` does not parse there.
        assert_eq!(field("gen"), "gen_");
        for reserved in [
            "async", "await", "dyn", "try", "abstract", "become", "box", "do", "final", "macro",
            "override", "priv", "typeof", "unsized", "virtual", "yield",
        ] {
            assert_eq!(field(reserved), format!("{reserved}_"));
        }
        // Weak keywords are ordinary identifiers.
        assert_eq!(field("union"), "union");
        assert_eq!(field("self"), "self_");
    }

    /// An emitter name is claimed verbatim, and only once.
    #[test]
    fn an_emitter_name_is_claimed_once() {
        let mut scope = NameScope::new(NameSpace::Values);
        assert_eq!(
            scope
                .claim_emitter_name("main")
                .map(|name| name.to_string()),
            Ok("main".to_owned())
        );
        assert!(scope.claim_emitter_name("main").is_err());
        // A source `main` then renders apart from the entry point.
        assert_eq!(scope.claim(src("main")).as_str(), "main_");
    }
}
