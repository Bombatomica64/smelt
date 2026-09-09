//! HIR construction context for TypeScript frontend lowering.

use std::collections::{HashMap, HashSet};

use crate::lowering::{ConstCollection, ConstLiteral, RestParam};
use smelt_hir::{
    Crate as HirCrate, ExprKind, Field, FunctionType, ItemId, Literal, TypeId, TypeParamDef,
};

/// Reusable value stored in a static object constant.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ObjectConstValue {
    /// Primitive literal value.
    Literal(Literal),
    /// JavaScript `RegExp` literal value with source pattern and flags.
    RegExp {
        /// Regex source without flag translation.
        pattern: String,
        /// JavaScript regular-expression flags.
        flags: String,
    },
    /// Literal array value.
    List(Vec<ObjectConstEntryValue>),
    /// Literal object value.
    Object(ObjectConst),
    /// Capturable HIR expression value.
    Expr(ExprKind),
}

/// A nested static value stored inside an object constant.
#[derive(Debug, Clone)]
pub struct ObjectConstEntryValue {
    /// Nested value payload.
    pub value: ObjectConstValue,
    /// HIR type of the nested value.
    pub ty: TypeId,
}

/// A static object-constant entry that can be recreated in later lowered bodies.
#[derive(Debug, Clone)]
pub struct ObjectConstEntry {
    /// Source object key.
    pub key: String,
    /// Value stored under the key.
    pub value: ObjectConstValue,
    /// HIR type of the value.
    pub value_ty: TypeId,
}

/// Static object-constant metadata preserved across TypeScript modules.
#[derive(Debug, Clone)]
pub struct ObjectConst {
    /// Entries in source order.
    pub entries: Vec<ObjectConstEntry>,
    /// HIR type of the object literal.
    pub ty: TypeId,
}

/// The length a parameter's *source* annotation demands of its argument.
///
/// TypeScript states array lengths in the type — `[A, B]` is exactly two
/// elements, `[T, ...T[]]` and `NonEmptyArray<T>` at least one — but HIR lowers
/// all three to a plain `Type::List`, so the requirement is erased. Overload
/// selection is where that matters: without it a fixed-arity tuple parameter
/// accepts any array and the earliest-declared overload wins calls TypeScript
/// would route elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamLength {
    /// A rest-less tuple `[A, B]`: exactly this many elements.
    Exact(usize),
    /// A required-prefix tuple `[T, ...T[]]` / `NonEmptyArray<T>`: at least
    /// this many elements.
    AtLeast(usize),
}

impl ParamLength {
    /// Whether an argument of proven length `len` satisfies this requirement.
    #[must_use]
    pub fn is_satisfied_by(self, len: usize) -> bool {
        match self {
            Self::Exact(want) => len == want,
            Self::AtLeast(want) => len >= want,
        }
    }
}

/// A TypeScript overload signature attached to a concrete implementation item.
#[derive(Debug, Clone)]
pub struct OverloadSignature {
    /// Generic parameters and their TypeScript constraints/defaults.
    pub type_params: Vec<TypeParamDef>,
    /// Parameter types in source order.
    pub params: Vec<TypeId>,
    /// Per-parameter length requirement, parallel to [`Self::params`].
    ///
    /// `None` where the annotation states no length (`T[]`, `Array<T>`, a
    /// non-array type). See [`ParamLength`] for why this cannot be recovered
    /// from the lowered `params` types.
    pub param_lengths: Vec<Option<ParamLength>>,
    /// Source index of a rest parameter, when this overload declares one.
    pub rest: Option<usize>,
    /// Minimum number of arguments the rest parameter requires.
    ///
    /// `0` for an ordinary `...rest: T[]` (which accepts an empty tail), and
    /// `1` (or more) when the rest parameter was declared with a required
    /// prefix such as `NonEmptyArray<T>` (`[T, ...T[]]`). The frontend lowers
    /// `[T, ...T[]]` to a plain `Type::List(T)`, erasing the min-arity, so this
    /// field preserves it for overload selection: a `NonEmptyArray` rest must
    /// not match an empty rest tail vacuously.
    pub min_rest: usize,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    pub required_params: Option<usize>,
    /// Return type promised by this overload.
    pub return_ty: TypeId,
    /// Whether the signature describes an async function.
    pub is_async: bool,
}

/// Context for building HIR from TypeScript source.
///
/// Manages the crate structure and accumulates items during lowering.
#[derive(Debug)]
pub struct HirCtx {
    /// The HIR crate being constructed.
    pub krate: HirCrate,
    /// Exported aliases created by re-export declarations.
    pub export_aliases: HashMap<String, ItemId>,
    /// Exported item names grouped by source module path.
    pub module_exports: HashMap<String, HashMap<String, ItemId>>,
    /// Names that alias the ambient global object, grouped by source module path.
    ///
    /// Populated when a module binds the global object (`const g = globalThis;`
    /// or the portable `(typeof globalThis === 'object' && globalThis) || …`
    /// detection chain) so that an importer of that binding recognizes it as the
    /// global object too. Without this carry, `import { globalThis } from
    /// './globalThis'` would read members off a fresh empty record instead of
    /// resolving them against the modeled global.
    pub module_global_object_aliases: HashMap<String, HashSet<String>>,
    /// Exported object constants used as namespace-like API surfaces.
    pub object_namespaces: HashMap<String, HashMap<String, ItemId>>,
    /// Exported object constants with literal data values.
    pub object_consts: HashMap<String, ObjectConst>,
    /// Exported object constants whose values can be projected by `Object.values`.
    pub object_value_collections: HashMap<String, ConstCollection>,
    /// Exported array/set constants that can be inlined by later modules.
    pub const_collections: HashMap<String, ConstCollection>,
    /// Const-folded TypeScript `enum` member values visible to later modules.
    ///
    /// Keyed by enum name then member name. Populated when an `enum`
    /// declaration is lowered, and read by later modules so an imported enum's
    /// members still fold to their constant literal in `EnumName.Member` reads
    /// and `case EnumName.Member:` labels.
    pub enum_members: HashMap<String, HashMap<String, ConstLiteral>>,
    /// Function overload signatures visible to later TypeScript modules.
    pub overloads: HashMap<String, Vec<OverloadSignature>>,
    /// Rest-parameter metadata visible to later TypeScript modules.
    pub function_rests: HashMap<String, RestParam>,
    /// Functions whose source return contract is a JavaScript `Date` value.
    pub date_returning_functions: HashSet<ItemId>,
    /// Structural fields attached to type aliases visible to later modules.
    pub type_alias_fields: HashMap<smelt_hir::Symbol, Vec<Field>>,
    /// Interface heritage clauses visible to later modules for lazy field lookup.
    pub interface_extends: HashMap<smelt_hir::Symbol, Vec<crate::lowering::InterfaceHeritageRef>>,
    /// Value types declared by interface string index signatures.
    pub interface_index_values: HashMap<smelt_hir::Symbol, TypeId>,
    /// Value types declared by class string index signatures (`[k: string]: T`).
    ///
    /// Populated when a class declaration carries a `TSIndexSignature`, and read
    /// during member/keyed access so `instance[dynamicKey]` and
    /// `instance.declaredButUnnamedMember` resolve to the index signature's value
    /// type instead of failing as an unknown class field. Declared named fields
    /// still take precedence; the index value is only the fallback shape.
    pub class_index_values: HashMap<smelt_hir::Symbol, TypeId>,
    /// Interface call signatures visible to later modules.
    pub interface_call_signatures: HashMap<smelt_hir::Symbol, Vec<FunctionType>>,
    /// Interface construct signatures (`new (): T`) visible to later modules.
    pub interface_construct_signatures: HashMap<smelt_hir::Symbol, Vec<FunctionType>>,
    /// Structural fields attached to callable intersection types.
    pub callable_fields: HashMap<TypeId, Vec<Field>>,
    /// Type aliases whose source surface is a callable object intersection.
    pub callable_object_aliases: HashSet<smelt_hir::Symbol>,
    /// Modeled host constructor names the crate reassigns via
    /// `globalThis.<Name> =` somewhere (the host-global override pre-pass).
    ///
    /// Populated once, crate-wide, before any module lowers, so a write in one
    /// module (an `isBlob.spec.ts`) switches on the dynamic override machinery
    /// for reads/presence guards/`new` dispatch in the module that defines the
    /// predicate (`isBlob.ts`), even though that module lowers first. A name
    /// absent from this set keeps byte-identical presence folding and native
    /// construction (pay-for-use).
    pub written_host_globals: HashSet<String>,
    /// Project source files that are NOT part of the crate being lowered.
    ///
    /// Canonical paths of files the manifest's source roots contain (excludes
    /// already removed) that the dependency closure did not reach. Seeded once,
    /// crate-wide, before any module lowers, so the fact is independent of
    /// lowering order.
    ///
    /// It exists to answer one question at a type reference: "is this name
    /// imported from a module that this project owns but the crate does not
    /// have?" A reference like that has no declaration to resolve against, and
    /// the nominal `Type::Class` stand-in it used to get is the worst possible
    /// answer — a nominal class erases, an erased union member makes the whole
    /// union non-concrete, and the union reaches the emitter as `SmeltUnknown`
    /// far from the reference with nothing naming the alias. Hono's five-arm
    /// `ResponseHeadersInit` erased exactly that way for weeks. Naming it at
    /// the reference is the difference between a one-line diagnostic and a
    /// campaign.
    ///
    /// Deliberately only the closure GAP: a name imported from a module the
    /// crate does have, from a bare package (`type-fest`), or from a module the
    /// manifest excludes keeps the nominal fallback. See
    /// `blocker-logs/standards-generic-arm-and-typeof-indexed-alias.md`.
    pub project_sources_outside_crate: HashSet<String>,
    /// Rust-facing class names for source class names that are AMBIGUOUS across
    /// the crate, keyed by module path then by source class name.
    ///
    /// Class identity in HIR is the class's name symbol (`Type::Class { name }`),
    /// and method resolution goes from that symbol back to the class item. Two
    /// modules exporting a class of the same name therefore interned ONE symbol
    /// for two different classes, and every method of the loser reported
    /// "unknown class method" — Hono's two `Node` classes
    /// (`router/trie-router/node.ts` and `router/reg-exp-router/node.ts`) are
    /// the case that found it, and it stopped the router slice transpiling
    /// outright.
    ///
    /// The transpiler computes this before any module lowers, so the answer
    /// cannot depend on lowering order: a name declared by exactly one module
    /// is absent here and keeps its bare spelling (so every existing golden is
    /// byte-identical), and a name declared by several gets the ordinal suffix
    /// scheme `manifest_module_names` already uses for module bodies — the last
    /// declaring module keeps the bare name, earlier ones become `Node_1`,
    /// `Node_2`, ...
    ///
    /// Only the RUST RENDERING changes. The source spelling stays the recorded
    /// original name (`krate.names`), because that is what reflection reads:
    /// `instanceof` and `__smelt_class` must still answer `Node`.
    pub class_renames: HashMap<String, HashMap<String, String>>,
    /// Crate-unique module identity for each module path.
    ///
    /// A module-private helper's Rust item name has to stay distinct after every
    /// source module is emitted into ONE generated crate — several modules
    /// legitimately spell a helper `lazyImplementation` — so the name is
    /// qualified by its module. It used to be qualified by `self.path`, the path
    /// the compiler was handed, which is absolute in a manifest build: the same
    /// TypeScript then emitted
    /// `is_short__module__home_user_project_src_main_ts` in one checkout and a
    /// different name in another, so generated output was not reproducible and
    /// no golden could cover the shape (H55).
    ///
    /// The transpiler fills this from `manifest_module_names`, the collision-free
    /// module identities it already computes for module BODIES (`node`,
    /// `node_1`, ...), so the qualifier is the same everywhere the crate is
    /// built. Empty when a module is lowered on its own (a unit test,
    /// `dump-hir`), and then the path is used as before.
    pub module_identities: HashMap<String, String>,
}

impl HirCtx {
    /// Create a new empty HIR context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
            export_aliases: HashMap::new(),
            module_exports: HashMap::new(),
            module_global_object_aliases: HashMap::new(),
            object_namespaces: HashMap::new(),
            object_consts: HashMap::new(),
            object_value_collections: HashMap::new(),
            const_collections: HashMap::new(),
            enum_members: HashMap::new(),
            overloads: HashMap::new(),
            function_rests: HashMap::new(),
            date_returning_functions: HashSet::new(),
            type_alias_fields: HashMap::new(),
            interface_extends: HashMap::new(),
            interface_index_values: HashMap::new(),
            class_index_values: HashMap::new(),
            interface_call_signatures: HashMap::new(),
            interface_construct_signatures: HashMap::new(),
            callable_fields: HashMap::new(),
            callable_object_aliases: HashSet::new(),
            written_host_globals: HashSet::new(),
            project_sources_outside_crate: HashSet::new(),
            class_renames: HashMap::new(),
            module_identities: HashMap::new(),
        }
    }
}

impl Default for HirCtx {
    /// Create a new HIR context (same as `new`).
    fn default() -> Self {
        Self::new()
    }
}
