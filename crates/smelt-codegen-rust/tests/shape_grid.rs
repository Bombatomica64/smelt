//! Generated callback-generics **shape grid**.
//!
//! ## Why this exists
//!
//! The rescued fixture corpus (`tests/fixtures/callback_generics/`) is a
//! hundred programs that agents *hand-wrote while guessing* at adverse shapes.
//! That guessing found five real miscompiles, so the shapes matter — but the
//! next defect will sit in whichever cell nobody thought to write. The space of
//! callback-generics shapes is small and enumerable, so this module enumerates
//! it: it builds a [`Shape`] out of eight axes, renders each legal
//! combination as a small TypeScript program, emits a crate from it and runs
//! `cargo check`. Zero rustc errors is the assertion.
//!
//! ## The axes
//!
//! Each is derived from a defect that actually shipped in PRs #202/#203, or
//! from a family the rescued fixtures cover by hand:
//!
//! | Axis | Values | Why |
//! | --- | --- | --- |
//! | [`Generics`] | none, one, two | generic arity of the callee |
//! | [`TPos`] | value param, callback param, callback return, both, nested | where the type parameter is reachable from |
//! | [`CbShape`] | required, optional, escaping, in a container, rest | how the callback parameter is spelled |
//! | [`Ret`] | none, bare, list, optional, union | the callee return: a call site using the *declared* rather than the substituted return type was one of the shipped defects |
//! | [`Mutable`] | none, scalar, composite | a `mutable_params` parameter that is *also* a monomorphizing composite was another |
//! | [`ArgKind`] | inline, named local, function item, erased cast, omitted, forwarded, caller parameter | what the call site actually hands over; "omitted with a default" was a third |
//! | [`Sites`] | one, two identical, two differing | whether two sites pin the callee's type parameter the same way |
//! | [`Caller`] | concrete, generic | whether the *calling* function is itself generic; a callback that reaches a callee from inside a generic frame is behind an `F{n}: ?Sized` bound that cannot unsize-coerce into a `&dyn Fn` parameter |
//!
//! [`Caller`] and [`ArgKind::CallerParam`] were added after a review found the
//! first seven axes could not express the family two of the rescued fixtures
//! were kept for (`concrete_callback_sunk_into_method`,
//! `generic_maker_forwarded_into_sink`): a *concrete* callback, inside a
//! *generic* caller, handed to a *non-generic* callee. Every other argument
//! kind builds its callback inside the call site, so the callee only ever saw
//! a closure of known concrete type; the forwarding sink is rendered with the
//! callee's own type parameters, so forwarding was only ever
//! generic-to-generic. The cell that reproduces the family is
//! `g0_val_req_rbare_m0_cparam_s1_cgen`.
//!
//! "Whether the callback type mentions a type parameter at all" is **not** a
//! separate axis: it is exactly [`TPos::Value`] with a non-`none`
//! [`Generics`] (a concrete callback inside a generic function), so a separate
//! boolean would have generated nothing but duplicates.
//!
//! ## What is pruned, and why
//!
//! Two prunings, both deliberate and both reported by [`grid_is_enumerable`].
//!
//! **Legality** ([`Shape::illegal_because`]) drops combinations that are not
//! well-typed TypeScript or that would generate a program with nothing to test.
//! Every rule names the reason; the test prints the tally per rule so a rule
//! that quietly eats half the grid is visible.
//!
//! **Blocking**: the full 8-way cross product is 47 250 cells, about twenty
//! hours of `cargo check`. Instead the grid is the union of six *blocks*, each
//! a full cross product over the axes that interact, with the remaining axes
//! pinned to a base value (see [`blocks`]). This is the standard
//! covering-design compromise: every 2-way interaction *within* a block is
//! exercised exhaustively, and the blocks are chosen so the interactions the
//! shipped defects lived in — callback shape against argument kind, mutability
//! against callback shape, return shape against generic arity, caller
//! genericity against argument kind — are all inside one block or another. Interactions *across* blocks (e.g. a `two`-generic
//! callee whose argument is an erased cast at two differing sites) are not
//! covered, and that is the honest limit of this grid.
//!
//! ## Tiers
//!
//! * [`shape_grid_fast`] — a greedy **pairwise covering subset**: a small set
//!   of generated shapes in which every pair of axis values that occurs
//!   anywhere in the grid occurs at least once (greedy, so near-minimal rather
//!   than provably minimal). 75 of the 467 shapes, **95s**
//!   measured on a 4-core sandbox. Runs per PR, in the `corpus` job of
//!   `.github/workflows/ci.yml`.
//! * [`shape_grid_full`] — every legal shape: 467 crates emitted and
//!   `cargo check`ed, **564s** (9.4 min) measured the same way, about 1.2s a
//!   shape. Runs nightly, in `.github/workflows/shape-grid.yml`. Ten minutes
//!   on every PR is a tier someone eventually switches off; pairwise on every
//!   PR plus the whole grid nightly keeps both honest.
//! * [`grid_is_enumerable`] — not `#[ignore]`d and compiles nothing: it asserts
//!   the size of the space, that names are unique, and that the fast tier
//!   really is a pairwise cover. It runs in a plain `cargo test`, so a change
//!   to the pruning rules shows up as a diff in an asserted number rather than
//!   as a silently smaller grid.
//!
//! Neither compiling tier parallelises its `cargo check`es: they share one
//! `CARGO_TARGET_DIR` so the emitted crates' dependencies and runtime prelude
//! compile once, and concurrent cargo invocations would serialize on that
//! directory's lock anyway.
//!
//! ```sh
//! just test-shape-grid          # fast tier
//! just test-shape-grid-full     # every legal shape
//! SMELT_GRID_ONLY=g1_both_req_rlist_mcomp_inline_s1_c0,g0_val_req_rbare_m0_cparam_s1_cgen \
//!   cargo test -p smelt-codegen-rust --test shape_grid -- --ignored
//! SMELT_GRID_DUMP=/tmp/grid \
//!   cargo test -p smelt-codegen-rust --test shape_grid -- --ignored shape_grid_full
//! ```
//!
//! `SMELT_GRID_DUMP` writes every generated program, the emitted `main.rs`, and
//! the captured `cargo check` output of every failing one, into a directory —
//! that is how a grid failure turns into a filed defect. `SMELT_GRID_ONLY`
//! takes a comma-separated list, so the failing shapes can be re-checked at
//! another commit in a single invocation.
//!
//! ## The generator is checked too
//!
//! A "defect" the grid reports is only evidence about the emitter if the
//! program it generated was well-typed to begin with. Every one of the 467
//! programs passes `tsc --strict --noEmit`, and the nightly workflow re-runs
//! that check so a renderer change cannot start manufacturing findings:
//!
//! ```sh
//! just dump-shape-grid /tmp/grid
//! tsc --strict --noEmit --target es2020 /tmp/grid/*.ts
//! ```

#![expect(
    clippy::too_many_lines,
    clippy::expect_used,
    clippy::similar_names,
    reason = "the renderer is one long match per axis; `crates_dir`/`crate_dir` are \
              the harness's own names; test setup fails fast on invalid inputs"
)]
#![expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "`Shape` is eight one-byte enums, so `&self` is indeed trivially copyable, \
              but its accessors are also used as function items — `iter().map(Shape::name)`, \
              `sort_by_key(Shape::name)`, `flat_map(Shape::axis_values)` — which require \
              an `fn(&Shape)` receiver; taking `self` would force a closure at every use"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "index and counter arithmetic over a grid whose size is a compile-time \
              constant asserted by `grid_is_enumerable`"
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

mod corpus_support;

use corpus_support::{
    cargo_check, emit_typescript_crate, rustc_error_codes, rustc_error_count, scratch_root,
};

// ---------------------------------------------------------------------------
// Axes
// ---------------------------------------------------------------------------

/// Generic arity of the callee under test.
///
/// `Two` always spends its second parameter `U` as the *callback's return
/// type*, which is what a second type parameter is for in practice (`map`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Generics {
    /// No type parameters: a concrete callee. The control row.
    None,
    /// One type parameter `T`.
    One,
    /// Two type parameters `T` (in the input) and `U` (the callback's return).
    Two,
}

/// Where the callee's type parameter is reachable from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TPos {
    /// Only a value parameter (`items: T[]`); the callback is concrete.
    Value,
    /// Only the callback's parameter (`cb: (v: T) => number`). No `T` value
    /// exists inside the body, so `T` is pinned solely by the call site's
    /// annotation.
    CbParam,
    /// Only the callback's return (`cb: (v: number) => T`).
    CbReturn,
    /// A value parameter *and* the callback.
    Both,
    /// Nested one callback deeper (`cb: (inner: (v: T) => T) => T`).
    Nested,
}

/// How the callback parameter is spelled on the callee.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CbShape {
    /// `cb: (…) => …` — the borrowed, required case.
    Required,
    /// `cb?: (…) => …`, with an `undefined` guard in the body.
    Optional,
    /// Required, but stored into a local list first, so it must be owned.
    Escaping,
    /// `cbs: ((…) => …)[]` — the adapter sits inside a composite.
    InContainer,
    /// `...cbs: ((…) => …)[]`.
    Rest,
}

/// The callee's return type, over the "payload" type (`number`, `T` or `U`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ret {
    /// `void`.
    None,
    /// The bare payload.
    Bare,
    /// `P[]` — a composite, which monomorphizes.
    List,
    /// `P | null`.
    Optional,
    /// `P | number` — a union that is only a union once `P` is substituted.
    Union,
}

/// Whether the callee takes a parameter it mutates, and of what kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mutable {
    /// No extra parameter.
    None,
    /// A scalar parameter reassigned in the body.
    Value,
    /// A `P[]` parameter pushed into: both a `mutable_params` entry and a
    /// monomorphizing composite. Exactly the pair that miscompiled.
    Composite,
}

/// What the call site hands over as the callback argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ArgKind {
    /// An inline arrow.
    Inline,
    /// An arrow bound to a `const` first.
    NamedLocal,
    /// A named top-level `function` item.
    FnItem,
    /// An `unknown` value cast to the callback type at the site.
    Erased,
    /// No argument at all (only legal against [`CbShape::Optional`]).
    Omitted,
    /// One local arrow passed to *two* callees with the same signature.
    Forwarded,
    /// The callback is a *parameter of the call site itself*, passed straight
    /// through to the callee. Every other kind builds the argument inside the
    /// call site, where its concrete closure type is known; this one is the
    /// only kind that hands the callee a value the caller is itself generic
    /// over.
    CallerParam,
}

/// Whether the *calling* function is generic in its own right.
///
/// The callee's genericity is [`Generics`]; this axis is about the frame the
/// call site sits in. It matters because the emitter infers a Rust type
/// parameter for a callback the caller only passes through
/// (`F0: Fn(..) + ?Sized`), and a `&F0` with a `?Sized` bound cannot
/// unsize-coerce into a `&dyn Fn` parameter — the E0277 the rescued fixtures
/// `concrete_callback_sunk_into_method` and `generic_maker_forwarded_into_sink`
/// were kept for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Caller {
    /// `export function useN(..)` — no type parameters of its own.
    Concrete,
    /// `export function useN<W>(spareN: W[], ..)` — a type parameter of the
    /// caller, in a value position, that nothing ever pins. The callback
    /// argument is therefore built (or received) inside a generic frame.
    Generic,
}

/// How many call sites there are, and whether they agree on the pin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sites {
    /// One site, pinning `T`/`U` to `number`.
    One,
    /// Two sites that both pin `number`.
    TwoSame,
    /// Two sites, one pinning `number` and one `string`.
    TwoDiff,
}

/// One cell of the grid: a full point in the seven-axis space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shape {
    generics: Generics,
    tpos: TPos,
    cb: CbShape,
    ret: Ret,
    mutable: Mutable,
    arg: ArgKind,
    sites: Sites,
    caller: Caller,
}

impl Generics {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::None => "g0",
            Self::One => "g1",
            Self::Two => "g2",
        }
    }

    /// The callee's type-parameter list, `""` when there is none.
    const fn params(self) -> &'static str {
        match self {
            Self::None => "",
            Self::One => "<T>",
            Self::Two => "<T, U>",
        }
    }
}

impl TPos {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::Value => "val",
            Self::CbParam => "cbp",
            Self::CbReturn => "cbr",
            Self::Both => "both",
            Self::Nested => "nest",
        }
    }
}

impl CbShape {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::Required => "req",
            Self::Optional => "opt",
            Self::Escaping => "esc",
            Self::InContainer => "vec",
            Self::Rest => "rest",
        }
    }
}

impl Ret {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::None => "rvoid",
            Self::Bare => "rbare",
            Self::List => "rlist",
            Self::Optional => "ropt",
            Self::Union => "runion",
        }
    }

    /// Renders the return type over the payload type `p`.
    fn ty(self, p: &str) -> String {
        match self {
            Self::None => "void".to_owned(),
            Self::Bare => p.to_owned(),
            Self::List => format!("{p}[]"),
            Self::Optional => format!("{p} | null"),
            // `number | number` is not a union; at a site that pins the
            // payload to `number` the union collapses, and spelling it out
            // would test the renderer's ability to write nonsense rather than
            // the emitter.
            Self::Union if p == "number" => "number".to_owned(),
            Self::Union => format!("{p} | number"),
        }
    }

    /// Renders `return …;` for a body whose payload value is bound to `out`.
    const fn return_stmt(self) -> &'static str {
        match self {
            Self::None => "  return;",
            Self::List => "  return [out];",
            Self::Bare | Self::Optional | Self::Union => "  return out;",
        }
    }
}

impl Mutable {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::None => "m0",
            Self::Value => "mval",
            Self::Composite => "mcomp",
        }
    }
}

impl ArgKind {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::NamedLocal => "local",
            Self::FnItem => "item",
            Self::Erased => "erased",
            Self::Omitted => "omit",
            Self::Forwarded => "fwd",
            Self::CallerParam => "cparam",
        }
    }
}

impl Caller {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::Concrete => "c0",
            Self::Generic => "cgen",
        }
    }

    /// The call site's own type-parameter list, `""` when it has none.
    const fn params(self) -> &'static str {
        match self {
            Self::Concrete => "",
            Self::Generic => "<W>",
        }
    }
}

impl Sites {
    /// Short token used in the shape's generated name.
    const fn token(self) -> &'static str {
        match self {
            Self::One => "s1",
            Self::TwoSame => "s2same",
            Self::TwoDiff => "s2diff",
        }
    }

    /// The `(T pin, U pin)` pair for each call site this shape emits.
    const fn pins(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::One => &[("number", "number")],
            Self::TwoSame => &[("number", "number"), ("number", "number")],
            Self::TwoDiff => &[("number", "number"), ("string", "string")],
        }
    }
}

/// A literal of type `ty`, used wherever the renderer needs a concrete value.
fn sample(ty: &str) -> &'static str {
    match ty {
        "string" => "\"a\"",
        _ => "1",
    }
}

/// An array literal of `ty` elements.
fn sample_list(ty: &str) -> &'static str {
    match ty {
        "string" => "[\"a\", \"b\"]",
        _ => "[1, 2, 3]",
    }
}

// ---------------------------------------------------------------------------
// Legality
// ---------------------------------------------------------------------------

impl Shape {
    /// The shape's stable name: one token per axis, in axis order.
    ///
    /// Also the emitted crate directory name and the `SMELT_GRID_ONLY` key.
    fn name(&self) -> String {
        format!(
            "{}_{}_{}_{}_{}_{}_{}_{}",
            self.generics.token(),
            self.tpos.token(),
            self.cb.token(),
            self.ret.token(),
            self.mutable.token(),
            self.arg.token(),
            self.sites.token(),
            self.caller.token()
        )
    }

    /// The eight `(axis, value)` labels of this shape, for covering analysis.
    const fn axis_values(&self) -> [(&'static str, &'static str); 8] {
        [
            ("generics", self.generics.token()),
            ("tpos", self.tpos.token()),
            ("cb", self.cb.token()),
            ("ret", self.ret.token()),
            ("mutable", self.mutable.token()),
            ("arg", self.arg.token()),
            ("sites", self.sites.token()),
            ("caller", self.caller.token()),
        ]
    }

    /// Returns the pruning rule that rejects this shape, or `None` if it is a
    /// well-typed program worth compiling.
    ///
    /// Every rule is a statement about TypeScript or about the program having
    /// something to test — never about the emitter being known-bad. A shape
    /// pruned here can never expose a defect, so the rules are deliberately few
    /// and each one is named in the grid's own report.
    fn illegal_because(&self) -> Option<&'static str> {
        if self.generics == Generics::None && self.tpos != TPos::Value {
            // With no type parameters there is no position for one to occupy;
            // `TPos::Value` is the single representative cell.
            return Some("no-generics: type-parameter position is vacuous");
        }
        if self.generics == Generics::Two && matches!(self.tpos, TPos::CbReturn | TPos::Nested) {
            // `U` *is* the callback's return, so "T in the callback return" and
            // "T nested one callback deeper" have no second spelling here.
            return Some("two-generics: U already occupies the callback return");
        }
        if self.ret == Ret::Union && self.generics == Generics::None {
            // `number | number` is not a union.
            return Some("union return needs a type parameter to be a union");
        }
        if self.ret != Ret::None && self.payload_source().is_none() {
            // No value of the payload type can be produced in the body, so the
            // function could not return one.
            return Some("return type unreachable: no payload value in scope");
        }
        if self.mutable == Mutable::Composite && self.payload_source().is_none() {
            // The mutable composite is a `P[]` that gets pushed into.
            return Some("mutable composite needs a payload value to push");
        }
        if self.arg == ArgKind::Omitted && self.cb != CbShape::Optional {
            return Some("omitted argument needs an optional parameter");
        }
        if self.cb == CbShape::Optional
            && self.ret != Ret::None
            && self.callback_free_payload().is_none()
        {
            // The `cb === undefined` arm must still return a payload.
            return Some("optional callback needs a callback-free payload to return");
        }
        if self.sites == Sites::TwoDiff && self.generics == Generics::None {
            return Some("two differing sites need a type parameter to differ on");
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl Shape {
    /// The payload type as spelled in the callee's own signature.
    const fn payload(&self) -> &'static str {
        match self.generics {
            Generics::None => "number",
            Generics::One => "T",
            Generics::Two => "U",
        }
    }

    /// The payload type as pinned at a call site (`t`/`u` are the site's pins).
    const fn payload_at<'a>(&self, t_ty: &'a str, u_ty: &'a str) -> &'a str {
        match self.generics {
            Generics::None => "number",
            Generics::One => t_ty,
            Generics::Two => u_ty,
        }
    }

    /// Whether the callee takes a list value parameter (`items`) rather than a
    /// scalar `count`.
    fn has_items(&self) -> bool {
        self.generics == Generics::None || matches!(self.tpos, TPos::Value | TPos::Both)
    }

    /// The element type of the `items` parameter as spelled in the signature.
    const fn elem(&self) -> &'static str {
        match self.generics {
            Generics::None => "number",
            Generics::One | Generics::Two => "T",
        }
    }

    /// The callback's function type, with `T`/`U` supplied by the caller so the
    /// same code renders both the declaration (`"T"`, `"U"`) and the concrete
    /// type at a call site (`"number"`, `"string"`).
    fn cb_ty(&self, t_ty: &str, u_ty: &str) -> String {
        match (self.generics, self.tpos) {
            (Generics::None, TPos::Value | TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) | (Generics::One, TPos::Value) => {
                "(v: number) => number".to_owned()
            }
            (Generics::One, TPos::CbParam) => format!("(v: {t_ty}) => number"),
            (Generics::One, TPos::CbReturn) => format!("(v: number) => {t_ty}"),
            (Generics::One, TPos::Both) => format!("(v: {t_ty}) => {t_ty}"),
            (Generics::One, TPos::Nested) => format!("(inner: (v: {t_ty}) => {t_ty}) => {t_ty}"),
            (Generics::Two, TPos::Value) => format!("(v: number) => {u_ty}"),
            (Generics::Two, TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) => {
                format!("(v: {t_ty}) => {u_ty}")
            }
        }
    }

    /// An arrow expression inhabiting [`Shape::cb_ty`] at the given pins.
    fn cb_arrow(&self, t_ty: &str, u_ty: &str) -> String {
        match (self.generics, self.tpos) {
            (Generics::None, TPos::Value | TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) | (Generics::One, TPos::Value) => {
                "(v: number) => v + 1".to_owned()
            }
            (Generics::One, TPos::CbParam) => format!("(v: {t_ty}) => 1"),
            (Generics::One, TPos::CbReturn) => format!("(v: number) => {}", sample(t_ty)),
            (Generics::One, TPos::Both) => format!("(v: {t_ty}) => v"),
            (Generics::One, TPos::Nested) => {
                format!("(inner: (v: {t_ty}) => {t_ty}) => inner({})", sample(t_ty))
            }
            (Generics::Two, TPos::Value) => format!("(v: number) => {}", sample(u_ty)),
            (Generics::Two, TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) => {
                format!("(v: {t_ty}) => {}", sample(u_ty))
            }
        }
    }

    /// A top-level `function` item inhabiting [`Shape::cb_ty`] at the given
    /// pins, for [`ArgKind::FnItem`].
    fn cb_item(&self, name: &str, t_ty: &str, u_ty: &str) -> String {
        match (self.generics, self.tpos) {
            (Generics::None, TPos::Value | TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) | (Generics::One, TPos::Value) => {
                format!("export function {name}(v: number): number {{ return v + 1; }}")
            }
            (Generics::One, TPos::CbParam) => {
                format!("export function {name}(v: {t_ty}): number {{ return 1; }}")
            }
            (Generics::One, TPos::CbReturn) => {
                format!(
                    "export function {name}(v: number): {t_ty} {{ return {}; }}",
                    sample(t_ty)
                )
            }
            (Generics::One, TPos::Both) => {
                format!("export function {name}(v: {t_ty}): {t_ty} {{ return v; }}")
            }
            (Generics::One, TPos::Nested) => format!(
                "export function {name}(inner: (v: {t_ty}) => {t_ty}): {t_ty} {{ return inner({}); }}",
                sample(t_ty)
            ),
            (Generics::Two, TPos::Value) => format!(
                "export function {name}(v: number): {u_ty} {{ return {}; }}",
                sample(u_ty)
            ),
            (Generics::Two, TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) => {
                format!(
                    "export function {name}(v: {t_ty}): {u_ty} {{ return {}; }}",
                    sample(u_ty)
                )
            }
        }
    }

    /// An expression of the payload type obtained by *calling* the callback,
    /// or `None` when calling it cannot produce one.
    ///
    /// `cb` is the expression naming the callable (`cb`, `kept[0]`, `cbs[0]`).
    fn payload_from_callback(&self, cb: &str) -> Option<String> {
        match (self.generics, self.tpos) {
            // With no type parameters the callback returns `number`, which
            // *is* the payload; `Both` reaches the payload the same way, by
            // calling the callback on an element of `items`.
            (
                Generics::None,
                TPos::Value | TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested,
            )
            | (Generics::One | Generics::Two, TPos::Both) => Some(format!("{cb}(items[0])")),
            // The callback's own parameter is `number` here, so a literal
            // suffices to drive it.
            (Generics::One, TPos::CbReturn) | (Generics::Two, TPos::Value) => {
                Some(format!("{cb}(1)"))
            }
            (Generics::One, TPos::Nested) => Some(format!("{cb}((v: T) => v)")),
            // `One`/`Value`: the callback yields `number`, not `T`.
            // `*`/`CbParam`: no `T` value exists, so it cannot even be called.
            // `Two`/`CbReturn` and `Two`/`Nested` are pruned as illegal, but
            // `illegal_because` calls this before pruning, so they are listed.
            (Generics::One, TPos::Value | TPos::CbParam)
            | (Generics::Two, TPos::CbParam | TPos::CbReturn | TPos::Nested) => None,
        }
    }

    /// An expression of the payload type that does **not** go through the
    /// callback, needed by the `cb === undefined` arm of an optional callback.
    const fn callback_free_payload(&self) -> Option<&'static str> {
        match (self.generics, self.tpos) {
            (Generics::None, TPos::Value | TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) | (Generics::One, TPos::Value | TPos::Both) => {
                Some("items[0]")
            }
            (Generics::One, TPos::CbParam | TPos::CbReturn | TPos::Nested)
            | (Generics::Two, TPos::Value | TPos::CbParam | TPos::CbReturn | TPos::Both | TPos::Nested) => None,
        }
    }

    /// Whether the callback can be invoked inside the callee body at all.
    fn callback_is_callable(&self) -> bool {
        self.tpos != TPos::CbParam
    }

    /// Any expression of the payload type available in the body, preferring the
    /// one that exercises the callback.
    fn payload_source(&self) -> Option<String> {
        self.payload_from_callback("cb")
            .or_else(|| self.callback_free_payload().map(str::to_owned))
    }

    /// Renders the callee named `name`.
    ///
    /// Parameter order is always `(value, mutable extra, callback)` so the rest
    /// parameter and the optional parameter are both legally last.
    fn render_callee(&self, name: &str) -> String {
        let cb_ty = self.cb_ty("T", "U");
        let payload = self.payload();

        let mut params: Vec<String> = Vec::new();
        if self.has_items() {
            params.push(format!("items: {}[]", self.elem()));
        } else {
            params.push("count: number".to_owned());
        }
        match self.mutable {
            Mutable::None => {}
            Mutable::Value => params.push("total: number".to_owned()),
            Mutable::Composite => params.push(format!("sink: {payload}[]")),
        }
        params.push(match self.cb {
            CbShape::Required | CbShape::Escaping => format!("cb: {cb_ty}"),
            CbShape::Optional => format!("cb?: {cb_ty}"),
            CbShape::InContainer => format!("cbs: ({cb_ty})[]"),
            CbShape::Rest => format!("...cbs: ({cb_ty})[]"),
        });

        let callable = match self.cb {
            CbShape::Required | CbShape::Optional => "cb",
            CbShape::Escaping => "kept[0]",
            CbShape::InContainer | CbShape::Rest => "cbs[0]",
        };

        let mut body: Vec<String> = Vec::new();

        // The `cb === undefined` arm comes first: everything after it may use
        // the callback unconditionally.
        if self.cb == CbShape::Optional {
            body.push("  if (cb === undefined) {".to_owned());
            if self.ret == Ret::None { body.push("    return;".to_owned()) } else {
                let free = self
                    .callback_free_payload()
                    .expect("legality guarantees a callback-free payload");
                let out = match self.ret {
                    Ret::List => format!("[{free}]"),
                    Ret::None | Ret::Bare | Ret::Optional | Ret::Union => free.to_owned(),
                };
                body.push(format!("    return {out};"));
            }
            body.push("  }".to_owned());
        }

        if self.cb == CbShape::Escaping {
            // Storing the callback in a list makes it escape the frame, so it
            // has to be owned rather than borrowed.
            body.push(format!("  const kept: ({cb_ty})[] = [cb];"));
        }

        if !self.callback_is_callable() {
            // `TPos::CbParam`: no `T` exists in the body, so the only thing the
            // callee can do with the callback is hold it. The interesting work
            // is entirely on the call-site side, which still has to build and
            // pass an adapter for a type parameter nothing else pins.
            body.push(format!("  const held = {callable};"));
        }

        if self.mutable == Mutable::Value {
            body.push("  total = total + 1;".to_owned());
        }

        if self.callback_is_callable() && self.payload_from_callback(callable).is_none() {
            // `One`/`Value`: a *concrete* callback inside a generic function.
            // Its result is not the payload, so it is consumed by a branch —
            // an unused local could be optimized away, and then the shape
            // would not be testing anything.
            body.push(format!("  const touched: number = {callable}(1);"));
            body.push("  if (touched < 0) {".to_owned());
            body.push(format!("  {}", self.ret_stmt_with_payload()));
            body.push("  }".to_owned());
        }

        if let Some(out) = self
            .payload_from_callback(callable)
            .or_else(|| self.callback_free_payload().map(str::to_owned))
        {
            body.push(format!("  const out: {payload} = {out};"));
        }
        if self.mutable == Mutable::Composite {
            body.push("  sink.push(out);".to_owned());
        }
        if self.ret != Ret::None {
            body.push(self.ret.return_stmt().to_owned());
        }

        format!(
            "export function {name}{}({}): {} {{\n{}\n}}",
            self.generics.params(),
            params.join(", "),
            self.ret.ty(payload),
            body.join("\n")
        )
    }

    /// The early-return used inside the `touched` branch, which runs *before*
    /// `out` is bound and so must re-derive its payload.
    fn ret_stmt_with_payload(&self) -> String {
        let Some(free) = self.callback_free_payload() else {
            return "  return;".to_owned();
        };
        match self.ret {
            Ret::None => "  return;".to_owned(),
            Ret::List => format!("  return [{free}];"),
            Ret::Bare | Ret::Optional | Ret::Union => format!("  return {free};"),
        }
    }

    /// The early return of a generic call site's `W` guard.
    ///
    /// It runs before the callee is ever called, so it cannot borrow a payload
    /// from the call: it returns a literal of the site's own return type,
    /// which is the callee return already pinned to this site's `payload_at`.
    fn site_early_return(&self, payload_at: &str) -> String {
        match self.ret {
            Ret::None => "  return;".to_owned(),
            Ret::List => format!("  return {};", sample_list(payload_at)),
            Ret::Optional => "  return null;".to_owned(),
            Ret::Bare | Ret::Union => format!("  return {};", sample(payload_at)),
        }
    }

    /// Renders one call site: any top-level declarations it needs, plus the
    /// `useN` function that performs the call(s).
    fn render_site(&self, idx: usize, t_ty: &str, u_ty: &str) -> (Vec<String>, String) {
        let mut decls: Vec<String> = Vec::new();
        let mut prelude: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();
        let mut site_params: Vec<String> = Vec::new();
        if self.caller == Caller::Generic {
            // A type parameter of the *call site*, in a value position.
            // Nothing ever pins it — `useN` is exported and never called — so
            // the emitted Rust caller stays generic, which is the frame the
            // callback argument is then built or received in.
            site_params.push(format!("spare{idx}: W[]"));
        }
        let elem_at = if self.generics == Generics::None {
            "number"
        } else {
            t_ty
        };
        let payload_at = self.payload_at(t_ty, u_ty);

        if self.has_items() {
            prelude.push(format!(
                "  const items{idx}: {elem_at}[] = {};",
                sample_list(elem_at)
            ));
            args.push(format!("items{idx}"));
        } else {
            args.push("3".to_owned());
        }
        match self.mutable {
            Mutable::None => {}
            Mutable::Value => args.push("0".to_owned()),
            Mutable::Composite => {
                prelude.push(format!("  const sink{idx}: {payload_at}[] = [];"));
                args.push(format!("sink{idx}"));
            }
        }

        let cb_arg: Option<String> = match self.arg {
            ArgKind::Inline => Some(self.cb_arrow(t_ty, u_ty)),
            ArgKind::NamedLocal | ArgKind::Forwarded => {
                prelude.push(format!("  const f{idx} = {};", self.cb_arrow(t_ty, u_ty)));
                Some(format!("f{idx}"))
            }
            ArgKind::FnItem => {
                decls.push(self.cb_item(&format!("item{idx}"), t_ty, u_ty));
                Some(format!("item{idx}"))
            }
            ArgKind::Erased => {
                site_params.push("raw: unknown".to_owned());
                // Deliberately *unparenthesized*: a parenthesized `as`
                // expression in argument position is a separate, unrelated
                // frontend gap ("call argument kind is not lowered yet:
                // ParenthesizedExpression") that would otherwise mask this
                // axis in every cell it appears in.
                Some(format!("raw as {}", self.cb_ty(t_ty, u_ty)))
            }
            ArgKind::CallerParam => {
                // The only kind where the argument is a value the *caller* is
                // generic over: the emitter infers `F0: Fn(..) + ?Sized` for a
                // callback parameter it merely passes on, and `&F0` cannot
                // unsize-coerce into a callee that took `&dyn Fn`.
                site_params.push(format!("cbp{idx}: {}", self.cb_ty(t_ty, u_ty)));
                Some(format!("cbp{idx}"))
            }
            ArgKind::Omitted => None,
        };
        if let Some(arg) = cb_arg {
            args.push(match self.cb {
                CbShape::InContainer => format!("[{arg}]"),
                CbShape::Required | CbShape::Optional | CbShape::Escaping | CbShape::Rest => arg,
            });
        }

        let call = format!("run({})", args.join(", "));
        let mut body = prelude;
        if self.caller == Caller::Generic {
            // Keeps `W` load-bearing: a parameter the body never reads could
            // be dropped, and then the site would not be generic at all.
            body.push(format!("  if (spare{idx}.length < 0) {{"));
            body.push(format!("  {}", self.site_early_return(payload_at)));
            body.push("  }".to_owned());
        }
        if self.arg == ArgKind::Forwarded {
            // The same local reaches a *second* callee with the same signature:
            // both call sites must agree about how the argument is owned.
            let second = format!("runB({})", args.join(", "));
            body.push(format!("  {call};"));
            match self.ret {
                Ret::None => body.push(format!("  {second};")),
                Ret::Bare | Ret::List | Ret::Optional | Ret::Union => {
                    body.push(format!("  return {second};"));
                }
            }
        } else {
            match self.ret {
                Ret::None => body.push(format!("  {call};")),
                Ret::Bare | Ret::List | Ret::Optional | Ret::Union => {
                    body.push(format!("  return {call};"));
                }
            }
        }

        let use_fn = format!(
            "export function use{idx}{}({}): {} {{\n{}\n}}",
            self.caller.params(),
            site_params.join(", "),
            self.ret.ty(payload_at),
            body.join("\n")
        );
        (decls, use_fn)
    }

    /// Renders the complete TypeScript program for this shape.
    fn render(&self) -> String {
        let mut out = String::new();
        writeln!(out, "// Shape: {}", self.name()).expect("write to String");
        writeln!(
            out,
            "// Axes: {}",
            self.axis_values()
                .iter()
                .map(|(axis, value)| format!("{axis}={value}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .expect("write to String");
        writeln!(
            out,
            "// Generated by tests/shape_grid.rs; do not edit by hand."
        )
        .expect("write to String");
        writeln!(out, "{}", self.render_callee("run")).expect("write to String");
        if self.arg == ArgKind::Forwarded {
            writeln!(out, "{}", self.render_callee("runB")).expect("write to String");
        }
        for (idx, (t_ty, u_ty)) in self.sites.pins().iter().enumerate() {
            let (decls, use_fn) = self.render_site(idx, t_ty, u_ty);
            for decl in decls {
                writeln!(out, "{decl}").expect("write to String");
            }
            writeln!(out, "{use_fn}").expect("write to String");
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The grid
// ---------------------------------------------------------------------------

const ALL_GENERICS: [Generics; 3] = [Generics::None, Generics::One, Generics::Two];
const ALL_TPOS: [TPos; 5] = [
    TPos::Value,
    TPos::CbParam,
    TPos::CbReturn,
    TPos::Both,
    TPos::Nested,
];
const ALL_CB: [CbShape; 5] = [
    CbShape::Required,
    CbShape::Optional,
    CbShape::Escaping,
    CbShape::InContainer,
    CbShape::Rest,
];
const ALL_RET: [Ret; 5] = [Ret::None, Ret::Bare, Ret::List, Ret::Optional, Ret::Union];
const ALL_MUTABLE: [Mutable; 3] = [Mutable::None, Mutable::Value, Mutable::Composite];
const ALL_ARG: [ArgKind; 7] = [
    ArgKind::Inline,
    ArgKind::NamedLocal,
    ArgKind::FnItem,
    ArgKind::Erased,
    ArgKind::Omitted,
    ArgKind::Forwarded,
    ArgKind::CallerParam,
];
const ALL_SITES: [Sites; 3] = [Sites::One, Sites::TwoSame, Sites::TwoDiff];
const ALL_CALLER: [Caller; 2] = [Caller::Concrete, Caller::Generic];

/// A named sub-cross-product of the space.
///
/// The full 7-way product is 20 250 cells; each block is a full product over
/// the axes named in `varies`, with every other axis pinned to the block's
/// base. See the module docs for why the space is blocked rather than
/// truncated.
struct Block {
    /// Block name, printed in the grid report.
    name: &'static str,
    /// Which axes the block sweeps, for the report.
    varies: &'static str,
    /// Why these axes are swept together.
    why: &'static str,
    /// Every shape the block produces, legal or not.
    shapes: Vec<Shape>,
}

/// The base shape every block starts from and pins its non-swept axes to.
const BASE: Shape = Shape {
    generics: Generics::One,
    tpos: TPos::Both,
    cb: CbShape::Required,
    ret: Ret::Bare,
    mutable: Mutable::None,
    arg: ArgKind::Inline,
    sites: Sites::One,
    caller: Caller::Concrete,
};

/// Returns the blocks whose union is the grid.
fn blocks() -> Vec<Block> {
    let mut out = Vec::new();

    // Where the type parameter lives, against how the callback and the return
    // are spelled. This is the "does substitution reach every position" block:
    // the adapter-substitution defects (parameter substituted but body not,
    // return left unsubstituted, call site using the declared return type) all
    // lived in this product.
    let mut substitution_shapes = Vec::new();
    for generics in ALL_GENERICS {
        for tpos in ALL_TPOS {
            for cb in ALL_CB {
                for ret in ALL_RET {
                    substitution_shapes.push(Shape {
                        generics,
                        tpos,
                        cb,
                        ret,
                        ..BASE
                    });
                }
            }
        }
    }
    out.push(Block {
        name: "substitution",
        varies: "generics x tpos x cb x ret",
        why: "every position a type parameter can occupy, against every callback \
              spelling and every return shape",
        shapes: substitution_shapes,
    });

    // What the call site hands over, against how the parameter is spelled and
    // how many sites there are. This is the argument-ladder block: the
    // passthrough/borrowed/omitted branches all claim the same argument, and
    // only some orderings were ever reasoned about.
    let mut supply_shapes = Vec::new();
    for arg in ALL_ARG {
        for sites in ALL_SITES {
            for cb in ALL_CB {
                supply_shapes.push(Shape {
                    cb,
                    arg,
                    sites,
                    ..BASE
                });
            }
        }
    }
    out.push(Block {
        name: "supply",
        varies: "arg x sites x cb",
        why: "which branch of the argument ladder owns the callback argument, at \
              one and at two call sites",
        shapes: supply_shapes,
    });

    // Mutability against the callback spelling and the argument kind. A
    // parameter that is both a monomorphizing composite and mutably borrowed
    // was claimed by the passthrough branch and rendered by value.
    let mut mutation_shapes = Vec::new();
    for mutable in ALL_MUTABLE {
        for cb in ALL_CB {
            for arg in ALL_ARG {
                mutation_shapes.push(Shape {
                    mutable,
                    cb,
                    arg,
                    ret: Ret::List,
                    ..BASE
                });
            }
        }
    }
    out.push(Block {
        name: "mutation",
        varies: "mutable x cb x arg",
        why: "a mutable parameter that is also a monomorphizing composite, against \
              every callback spelling and argument kind",
        shapes: mutation_shapes,
    });

    // Pinning: how many sites, pinning what, against the return shape and the
    // generic arity. A call site using the callee's declared return type where
    // the call really evaluates to the substituted one shipped 6 x E0308.
    let mut pinning_shapes = Vec::new();
    for sites in ALL_SITES {
        for ret in ALL_RET {
            for generics in ALL_GENERICS {
                for tpos in [TPos::Value, TPos::Both, TPos::CbReturn] {
                    pinning_shapes.push(Shape {
                        generics,
                        tpos,
                        ret,
                        sites,
                        ..BASE
                    });
                }
            }
        }
    }
    out.push(Block {
        name: "pinning",
        varies: "sites x ret x generics x tpos",
        why: "two sites pinning one callee identically or differently, against the \
              return shape the sites must agree on",
        shapes: pinning_shapes,
    });

    // Caller genericity against what the call site hands over and how generic
    // the callee is. Every other block builds its callback argument *inside*
    // the call site, where the closure has a known concrete type, and pins the
    // call site itself to non-generic — so no other block can produce a
    // callback that reaches the callee already behind the caller's own
    // `F{n}: ?Sized` bound. That shape shipped: it is the `E0277` the rescued
    // fixtures `concrete_callback_sunk_into_method` and
    // `generic_maker_forwarded_into_sink` were kept for, and the cell that
    // reproduces it is `g0_val_req_rbare_m0_cparam_s1_cgen` — a concrete
    // callback, inside a generic caller, handed to a non-generic callee.
    //
    // `tpos` is swept over the two positions that stay legal at every generic
    // arity so the `Generics::None` (non-generic callee) row survives pruning;
    // pinning it to the base `Both` would have deleted exactly the row this
    // block exists for.
    let mut caller_shapes = Vec::new();
    for caller in ALL_CALLER {
        for arg in ALL_ARG {
            for generics in ALL_GENERICS {
                for tpos in [TPos::Value, TPos::Both] {
                    caller_shapes.push(Shape {
                        generics,
                        tpos,
                        arg,
                        caller,
                        ..BASE
                    });
                }
            }
        }
    }
    out.push(Block {
        name: "caller",
        varies: "caller x arg x generics x tpos",
        why: "a generic call site, against what it hands over and how generic the \
              callee it hands it to is",
        shapes: caller_shapes,
    });

    // Caller genericity against the callback *spelling*. Whether the callee
    // took its callback borrowed, optional, owned, or inside a container
    // decides whether it can accept a generic bound at all, so it is the axis
    // that decides whether a caller-generic argument coerces.
    let mut caller_shape_shapes = Vec::new();
    for caller in ALL_CALLER {
        for cb in ALL_CB {
            for arg in ALL_ARG {
                caller_shape_shapes.push(Shape {
                    cb,
                    arg,
                    caller,
                    ..BASE
                });
            }
        }
    }
    out.push(Block {
        name: "caller-shape",
        varies: "caller x cb x arg",
        why: "a generic call site against every callback spelling the callee can \
              declare",
        shapes: caller_shape_shapes,
    });

    out
}

/// Renders the block table: what each block sweeps and why.
///
/// Printed by [`grid_is_enumerable`] when a size lock trips, so whoever changed
/// the space sees the design they changed rather than a bare number.
fn block_summary() -> String {
    let mut out = String::new();
    for block in blocks() {
        writeln!(
            out,
            "  {} ({}): {} candidate shape(s) — {}",
            block.name,
            block.varies,
            block.shapes.len(),
            block.why
        )
        .expect("write to String");
    }
    out
}

/// The enumerated grid: every legal shape in stable name order, paired with the
/// tally of how many candidate shapes each pruning rule rejected.
type Grid = (Vec<Shape>, BTreeMap<&'static str, usize>);

/// Returns the deduplicated, legal grid in stable name order, together with the
/// tally of how many candidate shapes each pruning rule rejected.
fn grid() -> Grid {
    let mut kept: BTreeMap<String, Shape> = BTreeMap::new();
    let mut pruned: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for block in blocks() {
        for shape in block.shapes {
            let name = shape.name();
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(reason) = shape.illegal_because() {
                *pruned.entry(reason).or_default() += 1;
                continue;
            }
            kept.insert(name, shape);
        }
    }
    (kept.into_values().collect(), pruned)
}

/// One covered interaction: `(left axis index, left value, right axis index,
/// right value)`. Named so the covering sets below are readable types rather
/// than four-element tuples spelled out at every use.
type AxisPair = (usize, &'static str, usize, &'static str);

/// Greedily selects a small subset of `shapes` in which every pair of axis
/// values that occurs anywhere in `shapes` occurs at least once.
///
/// This is a standard greedy pairwise covering array: at each step it takes the
/// shape covering the most still-uncovered pairs, ties broken by name order, so
/// the result is deterministic. Pairwise is the right strength for the fast
/// tier because every defect the campaign shipped was an interaction between
/// exactly two axes.
fn pairwise_cover(shapes: &[Shape]) -> Vec<Shape> {
    let pairs_of = |shape: &Shape| -> Vec<AxisPair> {
        let values = shape.axis_values();
        let mut pairs = Vec::new();
        for (i, (_, left)) in values.iter().enumerate() {
            for (j, (_, right)) in values.iter().enumerate().skip(i + 1) {
                pairs.push((i, *left, j, *right));
            }
        }
        pairs
    };

    let mut uncovered: BTreeSet<AxisPair> =
        shapes.iter().flat_map(&pairs_of).collect();

    let mut chosen: Vec<Shape> = Vec::new();
    while !uncovered.is_empty() {
        let best = shapes
            .iter()
            .max_by_key(|shape| {
                pairs_of(shape)
                    .into_iter()
                    .filter(|pair| uncovered.contains(pair))
                    .count()
            })
            .copied()
            .expect("grid is non-empty while pairs remain uncovered");
        for pair in pairs_of(&best) {
            uncovered.remove(&pair);
        }
        chosen.push(best);
    }
    chosen.sort_by_key(Shape::name);
    chosen
}

// ---------------------------------------------------------------------------
// Expected failures
// ---------------------------------------------------------------------------

/// A generated shape that does not compile at HEAD.
///
/// **Every entry here is a live defect, not accepted behaviour.** The table
/// exists so the tier can be green in CI while the defects are triaged and
/// fixed separately; the grid still fails when a *new* shape breaks, when a
/// recorded one starts compiling, or when the set of rustc error codes it
/// produces changes.
struct GridExpected {
    /// [`Shape::name`] of the failing shape.
    name: &'static str,
    /// The rustc error codes observed, sorted and deduplicated, or `error` for
    /// a bare `error:` (including pre-`rustc` failures, which record `smelt`).
    codes: &'static [&'static str],
    /// What the defect is, in one line.
    note: &'static str,
}

/// Shapes known to fail at HEAD, grouped by defect family.
///
/// Recorded with error *codes* rather than counts: which diagnostics fire is
/// stable across toolchains, how rustc groups them is not.
const GRID_EXPECTED_FAILURES: &[GridExpected] = &[
    // -- Fixed: a union return that mentions a type parameter.
    //
    // Nineteen `runion` shapes used to be recorded here. The source is
    // `function run<T>(items: T[], cb: (v: T) => T): T | number`. Because the
    // return is a union mentioning `T` it has no concrete Rust spelling and
    // renders `SmeltUnknown`, while the parameters stayed monomorphized, so the
    // body returned a bare `T` against a `-> SmeltUnknown` signature (E0308).
    //
    // The disagreement was inside the erase verb itself: `coercion::erase` (the
    // operand-shaped twin of `coercion::erase_value`) passed a
    // `Type::TypeParam` operand through unchanged, which is right only when the
    // signature spelled that parameter `SmeltUnknown` too. It now asks
    // `current_function_has_type_param` — the same scope decision the signature
    // was rendered from — and converts a monomorphized `T` through the
    // `IntoSmeltUnknown` bound the signature declares. See the
    // `erases_a_monomorphized_type_parameter_at_an_erased_union_return`
    // regression test in `smelt-codegen-rust`.

    // Fixed: a borrowed callback packed into a container is now an escape.
    //
    // The ownership fixpoint (`compute_owned_callback_params` ->
    // `callback_param_escapes_locally`) counted packing a callback parameter
    // into a container literal only when the container ALSO erased. A
    // concretely typed `SmeltList<Rc<dyn Fn(..)>>` owns its elements just as
    // hard and carries no lifetime, so a borrowed `&dyn Fn` parameter placed
    // into one escaped (E0521). `statement_packs_callback_param_into_container`
    // makes it an escape independent of erasure and of the callee, so such a
    // parameter now enters its function as an owned handle. Pinned by
    // `borrowed_callback_packed_into_container_is_owned` and its rest twin in
    // `generics_tests`.
];

// ---------------------------------------------------------------------------
// Tiers
// ---------------------------------------------------------------------------

/// Emits and `cargo check`s every shape in `shapes`, reconciling the outcome
/// against [`GRID_EXPECTED_FAILURES`].
///
/// `full` selects whether records for shapes outside `shapes` are treated as
/// stale: only the full tier sees the whole grid, so only it can police the
/// table for rot.
fn run_grid(shapes: &[Shape], full: bool) {
    let root = scratch_root("smelt-shape-grid");
    let crates_dir = root.join("crates");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crates_dir).expect("create scratch crates dir");
    std::fs::create_dir_all(&target_dir).expect("create scratch target dir");

    // `SMELT_GRID_ONLY` takes one shape name or a comma-separated list, so a
    // triage run can re-check exactly the failing shapes — in another worktree,
    // at another commit — in a single invocation that pays for the emitted
    // crates' shared dependency build only once.
    let only: Option<Vec<String>> = std::env::var("SMELT_GRID_ONLY")
        .ok()
        .map(|value| value.split(',').map(|name| name.trim().to_owned()).collect());
    let dump = std::env::var("SMELT_GRID_DUMP").ok().map(std::path::PathBuf::from);
    if let Some(dir) = &dump {
        std::fs::create_dir_all(dir).expect("create dump dir");
    }

    let mut new_failures: Vec<String> = Vec::new();
    let mut unexpected_passes: Vec<String> = Vec::new();
    let mut drift: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for shape in shapes {
        let name = shape.name();
        if let Some(only_names) = &only
            && !only_names.contains(&name)
        {
            continue;
        }
        let source = shape.render();
        if let Some(dir) = &dump {
            std::fs::write(dir.join(format!("{name}.ts")), &source).expect("dump source");
        }
        checked += 1;
        let crate_dir = crates_dir.join(&name);
        let outcome = emit_typescript_crate(&name, &source, &crate_dir)
            .and_then(|()| {
                if let Some(dir) = &dump {
                    // The emitted Rust is what a defect report has to quote, and
                    // the scratch crate is deleted when the tier finishes.
                    drop(std::fs::copy(
                        crate_dir.join("src").join("main.rs"),
                        dir.join(format!("{name}.rs")),
                    ));
                }
                cargo_check(&crate_dir, &target_dir)
            });
        let expected = GRID_EXPECTED_FAILURES
            .iter()
            .find(|entry| entry.name == name);

        match (outcome, expected) {
            (Ok(()), None) => {}
            (Ok(()), Some(entry)) => unexpected_passes.push(format!(
                "{name}: recorded as failing ({}) but now COMPILES. Remove its \
                 GRID_EXPECTED_FAILURES record.",
                entry.note
            )),
            (Err(err), record) => {
                let mut codes = rustc_error_codes(&err);
                if codes.is_empty() {
                    // The failure came from the frontend or the emitter, before
                    // rustc ever ran.
                    codes.push("smelt".to_owned());
                }
                if let Some(dir) = &dump {
                    std::fs::write(dir.join(format!("{name}.err.txt")), &err).expect("dump error");
                }
                match record {
                    None => new_failures.push(format!(
                        "{name} [{}] ({} error(s))\n--- source ---\n{source}--- cargo check ---\n{}",
                        codes.join(", "),
                        rustc_error_count(&err),
                        err.lines().take(40).collect::<Vec<_>>().join("\n")
                    )),
                    Some(entry) => {
                        if codes != entry.codes {
                            drift.push(format!(
                                "{name}: recorded codes {:?}, observed {codes:?}\n    {}",
                                entry.codes,
                                err.lines().take(3).collect::<Vec<_>>().join("\n    ")
                            ));
                        }
                    }
                }
            }
        }
    }

    let stale: Vec<String> = if full && only.is_none() {
        GRID_EXPECTED_FAILURES
            .iter()
            .filter(|entry| !shapes.iter().any(|shape| shape.name() == entry.name))
            .map(|entry| format!("{}: no such shape in the grid", entry.name))
            .collect()
    } else {
        Vec::new()
    };

    drop(std::fs::remove_dir_all(&root));

    let mut report = String::new();
    if !new_failures.is_empty() {
        write!(
            report,
            "{} of {checked} generated shape(s) do not compile and are not recorded. \
             Each one is a defect no existing gate covers:\n\n{}\n",
            new_failures.len(),
            new_failures.join("\n\n")
        )
        .expect("write to String");
    }
    if !unexpected_passes.is_empty() {
        write!(
            report,
            "{} recorded failure(s) now compile:\n{}\n",
            unexpected_passes.len(),
            unexpected_passes.join("\n")
        )
        .expect("write to String");
    }
    if !drift.is_empty() {
        // Surfaced, not asserted, for the same reason the fixture tier does not
        // assert its error counts (see `ExpectedFailure::errors` in
        // `compile_corpus.rs`), and one more that is specific to this tier.
        //
        // A recorded-failing shape is already known broken. What is a stable
        // interface about it is that it FAILS -- not which rustc codes it
        // reports, and not the STAGE it fails at. The `smelt` marker means the
        // failure came from the frontend or emitter before rustc ran, and
        // whether a given broken shape reaches rustc at all can differ between
        // environments: this tier shares one CARGO_TARGET_DIR across every
        // emitted crate, so a build that fails for an environmental reason
        // produces text with no `error[E....]` in it and classifies as `smelt`.
        // Asserting on that made CI red while both local feature configurations
        // were green, which is a flaky gate rather than a signal.
        //
        // The two hard signals are kept: a recorded failure that starts
        // COMPILING fails this tier (someone fixed it -- update the record), and
        // a shape with no record that fails is a new defect. Both are below.
        // The first lines of the observed error are printed with each drift note
        // so the difference stays diagnosable from CI output alone.
        #[expect(
            clippy::print_stdout,
            reason = "the drift note is only useful in the tier's own output"
        )]
        {
            println!(
                "shape grid: {} recorded failure(s) changed their error codes:\n{}",
                drift.len(),
                drift.join("\n")
            );
        }
    }
    if !stale.is_empty() {
        write!(
            report,
            "{} stale record(s):\n{}\n",
            stale.len(),
            stale.join("\n")
        )
        .expect("write to String");
    }
    assert!(report.is_empty(), "{report}");
}

/// One `(axis name, axis value)` label, as [`Shape::axis_values`] yields them.
type AxisValue = (&'static str, &'static str);

/// Asserts the shape of the space itself: its size, the pruning tally, unique
/// names, and that the fast tier is a genuine pairwise cover.
///
/// Compiles nothing, so it runs in a plain `cargo test`. The asserted sizes are
/// deliberate locks: changing a pruning rule or a block must show up as a diff
/// in a number here, never as a silently smaller grid.
#[test]
fn grid_is_enumerable() {
    let (shapes, pruned) = grid();

    // Inspecting the space without paying for a single `cargo check`:
    // `SMELT_GRID_DUMP=<dir> cargo test --test shape_grid grid_is_enumerable`
    // writes every generated program out for review (and for `tsc`, which is
    // how the renderer itself is checked — a shape that is not valid
    // TypeScript would report as an emitter defect it is not).
    if let Ok(raw_dir) = std::env::var("SMELT_GRID_DUMP") {
        let dump_dir = std::path::PathBuf::from(raw_dir);
        std::fs::create_dir_all(&dump_dir).expect("create dump dir");
        for shape in &shapes {
            std::fs::write(dump_dir.join(format!("{}.ts", shape.name())), shape.render())
                .expect("dump source");
        }
    }
    let names: BTreeSet<String> = shapes.iter().map(Shape::name).collect();
    assert_eq!(names.len(), shapes.len(), "shape names must be unique");

    let fast = pairwise_cover(&shapes);
    let fast_names: BTreeSet<String> = fast.iter().map(Shape::name).collect();
    assert!(
        fast_names.is_subset(&names),
        "the fast tier must be a subset of the grid"
    );

    // Every 1-way axis value present in the grid must survive into the fast
    // tier: a pairwise cover implies it, and asserting it catches a broken
    // greedy selection directly.
    let axis_values = |set: &[Shape]| -> BTreeSet<AxisValue> {
        set.iter().flat_map(Shape::axis_values).collect()
    };
    assert_eq!(
        axis_values(&fast),
        axis_values(&shapes),
        "the fast tier must exercise every axis value the grid contains"
    );

    assert_eq!(
        (shapes.len(), fast.len()),
        (GRID_SIZE, FAST_TIER_SIZE),
        "grid size changed; update GRID_SIZE/FAST_TIER_SIZE and say why in the commit.\n\
         blocks:\n{}\n         pruning tally: {pruned:#?}",
        block_summary()
    );
}

/// Number of legal shapes in the grid. Locked by [`grid_is_enumerable`].
const GRID_SIZE: usize = 467;
/// Number of shapes in the pairwise fast tier. Locked by [`grid_is_enumerable`].
const FAST_TIER_SIZE: usize = 75;

/// Fast tier: a pairwise covering subset of the grid. Intended to run per PR.
#[test]
#[ignore = "slow: emits one crate per shape and runs cargo check; run via --ignored"]
fn shape_grid_fast() {
    let (shapes, _) = grid();
    run_grid(&pairwise_cover(&shapes), false);
}

/// Full tier: every legal shape in the grid. Intended to run nightly.
#[test]
#[ignore = "very slow: the whole shape grid; run nightly via --ignored"]
fn shape_grid_full() {
    let (shapes, _) = grid();
    run_grid(&shapes, true);
}
