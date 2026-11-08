//! HIR construction context for TypeScript frontend lowering.

use std::ops::Deref;
use std::rc::Rc;
use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

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
    /// Source object key, as Rust text (a lone surrogate written as U+FFFD).
    pub key: String,
    /// The key's exact UTF-16 code units when it holds a lone surrogate, which
    /// [`Self::key`] cannot spell.
    pub key_units: Option<Vec<u16>>,
    /// Value stored under the key.
    pub value: ObjectConstValue,
    /// HIR type of the value.
    pub value_ty: TypeId,
}

impl ObjectConstEntry {
    /// The key as the HIR string literal a lowered body reads it as.
    #[must_use]
    pub fn key_literal(&self) -> Literal {
        self.key_units.clone().map_or_else(|| Literal::String(self.key.clone()), Literal::StringUnits)
    }
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
/// elements, `[T, ...T[]]` at least one — but HIR lowers both to a plain
/// `Type::List`, so the requirement is erased. Overload
/// selection is where that matters: without it a fixed-arity tuple parameter
/// accepts any array and the earliest-declared overload wins calls TypeScript
/// would route elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamLength {
    /// A rest-less tuple `[A, B]`: exactly this many elements.
    Exact(usize),
    /// A required-prefix tuple `[T, ...T[]]`: at least this many elements.
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
    /// prefix such as `[T, ...T[]]`. The frontend lowers that to a plain
    /// `Type::List(T)`, erasing the min-arity, so this field preserves it for
    /// overload selection: a required prefix must not match an empty rest tail
    /// vacuously.
    pub min_rest: usize,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    pub required_params: Option<usize>,
    /// Return type promised by this overload.
    pub return_ty: TypeId,
    /// Whether the signature describes an async function.
    pub is_async: bool,
}

/// One source module's identity as a key of the crate-wide export tables.
///
/// The canonical path when the module exists on disk, so `./a.ts`, `a.ts` and
/// the absolute spelling of one file are one key; otherwise the path as given,
/// without a leading `./` (a module lowered on its own in a unit test).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleKey(String);

impl ModuleKey {
    /// The key of the module at `path`.
    #[must_use]
    pub fn of_path(path: &str) -> Self {
        if let Ok(canonical) = std::fs::canonicalize(path) {
            return Self(canonical.display().to_string());
        }
        // Not on disk: normalize lexically, so `src/./number.ts` (an
        // importer's `./number` joined onto its directory) and `src/number.ts`
        // are one key.
        let mut normalized = std::path::PathBuf::new();
        for component in std::path::Path::new(path).components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !normalized.pop() {
                        normalized.push("..");
                    }
                }
                other => normalized.push(other),
            }
        }
        Self(normalized.display().to_string())
    }
}

/// A name as one module exports it: the key of every crate-wide table that
/// carries facts from a declaring module to its importers.
///
/// Two modules may export the same name (`a.ts` and `b.ts` both
/// `export const DEFAULTS`), so a table keyed by the bare name answers with
/// whichever module wrote last. An importer instead asks for the name in the
/// module its `from` clause resolves to (audit finding A2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QualifiedExport {
    /// The exporting module.
    pub module: ModuleKey,
    /// The exported name (dotted for a static hung off an export: `partial.placeholder`).
    pub name: String,
}

/// A crate-wide table of facts about exported names, keyed by [`QualifiedExport`].
pub type ExportTable<V> = Published<HashMap<QualifiedExport, V>>;

#[cfg(test)]
impl<V> Published<HashMap<QualifiedExport, V>> {
    /// The entry some module published for `name`, for tests that lower one
    /// module and inspect what it exported.
    pub(crate) fn exported(&self, name: &str) -> Option<&V> {
        self.0.iter().find(|(key, _)| key.name == name).map(|(_, value)| value)
    }
}

/// A table every module has published, shared with the builders that read it.
///
/// A builder takes a snapshot when it starts: taking one shares the table, and the
/// first write on either side copies it, so a builder never sees what a later
/// write publishes.
#[derive(Debug, Default)]
pub struct Published<T>(Rc<T>);

impl<T> Clone for Published<T> {
    /// Share the table.
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> Deref for Published<T> {
    type Target = T;

    /// Read the table.
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> Published<T> {
    /// Write to this side's table, detaching it from any snapshot still sharing it.
    pub fn write(&mut self) -> &mut T {
        Rc::make_mut(&mut self.0)
    }
}

/// The narrowing a type predicate function proves for one of its parameters.
#[derive(Debug, Clone, Copy)]
pub struct ItemPredicate {
    /// `asserts value is T` rather than `value is T`.
    pub is_assertion: bool,
    /// Positional index of the parameter it narrows.
    pub param_index: usize,
    /// The type it proves.
    pub target: TypeId,
}

/// Context for building HIR from TypeScript source.
///
/// Manages the crate structure and accumulates items during lowering.
#[derive(Debug)]
pub struct HirCtx {
    /// The HIR crate being constructed.
    pub krate: HirCrate,
    /// Exported aliases created by re-export declarations, keyed by the
    /// module that publishes them.
    pub export_aliases: HashMap<QualifiedExport, ItemId>,
    /// Every module a builder has been created for, so an importer can tell
    /// which of a specifier's candidate paths names a crate module.
    pub known_modules: HashSet<ModuleKey>,
    /// Exported item names grouped by source module path.
    pub module_exports: HashMap<String, HashMap<String, ItemId>>,
    /// What each exported `value is T` or `asserts value is T` function proves.
    ///
    /// A type predicate is a fact of the signature, so it reaches an importer
    /// with the item, under whatever local name the importer gives it.
    pub item_predicates: HashMap<ItemId, ItemPredicate>,
    /// Names that alias the ambient global object, grouped by source module path.
    ///
    /// Populated when a module binds the global object (`const g = globalThis;`
    /// or the portable `(typeof globalThis === 'object' && globalThis) || …`
    /// detection chain) so that an importer of that binding recognizes it as the
    /// global object too. Without this carry, `import { globalThis } from
    /// './globalThis'` would read members off a fresh empty record instead of
    /// resolving them against the modeled global.
    pub module_global_object_aliases: HashMap<String, HashSet<String>>,
    /// Exported object constants used as namespace-like API surfaces, each with
    /// its entries in source order.
    pub object_namespaces: ExportTable<IndexMap<String, ItemId>>,
    /// Exported object constants with literal data values.
    pub object_consts: ExportTable<ObjectConst>,
    /// Exported object constants whose values can be projected by `Object.values`.
    pub object_value_collections: ExportTable<ConstCollection>,
    /// Exported array/set constants that can be inlined by later modules.
    pub const_collections: ExportTable<ConstCollection>,
    /// Const-folded TypeScript `enum` member values visible to later modules.
    ///
    /// Keyed by the exporting enum then member name. Populated when an `enum`
    /// declaration is lowered, and read by later modules so an imported enum's
    /// members still fold to their constant literal in `EnumName.Member` reads
    /// and `case EnumName.Member:` labels.
    pub enum_members: ExportTable<HashMap<String, ConstLiteral>>,
    /// Function overload signatures visible to later TypeScript modules.
    pub overloads: ExportTable<Vec<OverloadSignature>>,
    /// Rest-parameter metadata visible to later TypeScript modules.
    pub function_rests: ExportTable<RestParam>,
    /// Functions whose source return contract is a JavaScript `Date` value.
    pub date_returning_functions: HashSet<ItemId>,
    /// Structural fields attached to type aliases visible to later modules.
    pub type_alias_fields: Published<HashMap<smelt_hir::Symbol, Vec<Field>>>,
    /// Interface heritage clauses visible to later modules for lazy field lookup.
    pub interface_extends: Published<HashMap<smelt_hir::Symbol, Vec<crate::lowering::InterfaceHeritageRef>>>,
    /// Value types declared by interface string index signatures.
    pub interface_index_values: Published<HashMap<smelt_hir::Symbol, TypeId>>,
    /// Declared type parameters of every class the crate lowers, by name symbol.
    ///
    /// CRATE-scoped rather than per-module, and filled by the crate-wide
    /// predeclaration pass before any module body is lowered, because a type
    /// reference must take a generic class's type-parameter DEFAULTS even when
    /// the declaring module has not been lowered yet. Dependency cycles through
    /// barrel files routinely put a consumer first, and within one module
    /// TypeScript hoists class types anyway, so "the class is already an
    /// `Item::Class`" is not a condition the defaults rule can depend on.
    ///
    /// Keyed by the interned spelling of the declared name, which is what an
    /// unqualified type reference to it resolves to.
    pub class_type_params: HashMap<smelt_hir::Symbol, Vec<TypeParamDef>>,
    /// Each class declaration's constructor parameter types, lowered in the
    /// class's own type-parameter scope by a prepass
    /// (`collect_class_constructor_param_types`), keyed like
    /// `class_type_params`. A `new C(..)` lowered while `C` is still pending
    /// (inside its own body, or earlier in an import cycle) types its
    /// arguments from these.
    pub class_constructor_params: HashMap<smelt_hir::Symbol, Vec<TypeId>>,
    /// The interface synthesized for a mapped utility over record `X` (see
    /// `ty/annotations/derived_records.rs`), keyed by `X`'s type and the
    /// derived property list. Crate-wide so every module spelling the same
    /// derivation names the same struct.
    pub derived_records: HashMap<(TypeId, Vec<(smelt_hir::Symbol, bool)>), TypeId>,
    /// Type parameters a callable item DECLARES in source but whose HIR item
    /// does not carry (its `Function::type_params` is empty).
    ///
    /// Class methods and static methods do not yet
    /// materialize their own generics: codegen renders each such parameter as
    /// `SmeltUnknown` in the definition. A call to one must still know which
    /// type parameters in the declared signature are the CALLEE's own, so the
    /// call's result substitutes them (to the erased type the definition really
    /// returns) instead of leaking a raw `TypeParam` that, being identified by
    /// name, would alias a same-named parameter of the caller. Keyed by the
    /// callee's item id.
    pub erased_item_type_params: HashMap<ItemId, Vec<TypeParamDef>>,
    /// Value types declared by class string index signatures (`[k: string]: T`).
    ///
    /// Populated when a class declaration carries a `TSIndexSignature`, and read
    /// during member/keyed access so `instance[dynamicKey]` and
    /// `instance.declaredButUnnamedMember` resolve to the index signature's value
    /// type instead of failing as an unknown class field. Declared named fields
    /// still take precedence; the index value is only the fallback shape.
    pub class_index_values: Published<HashMap<smelt_hir::Symbol, TypeId>>,
    /// Interface call signatures visible to later modules.
    pub interface_call_signatures: Published<HashMap<smelt_hir::Symbol, Vec<FunctionType>>>,
    /// Interface construct signatures (`new (): T`) visible to later modules.
    pub interface_construct_signatures: Published<HashMap<smelt_hir::Symbol, Vec<FunctionType>>>,
    /// Structural fields attached to callable intersection types.
    pub callable_fields: Published<HashMap<TypeId, Vec<Field>>>,
    /// Type aliases whose source surface is a callable object intersection.
    pub callable_object_aliases: Published<HashSet<smelt_hir::Symbol>>,
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
    pub type_renames: HashMap<String, HashMap<String, String>>,
    /// Every type name some module of the crate declares at its top level.
    ///
    /// A block-local declaration (a class in a test callback) must not take
    /// one of these spellings as its Rust name: the top-level declaration may
    /// lower later, in another module, and claim the same symbol, which MIR
    /// rejects as two classes of one name (audit finding C12; ts-pattern's
    /// `class A` in a test callback and at the top of another test file).
    pub top_level_type_names: HashSet<String>,
    /// Every member name a class, interface or object type of a lowered module
    /// declares, the cheap first question of "does the receiver own this call".
    pub declared_member_names: HashSet<String>,
    /// Class expressions whose name is visible only inside their own body.
    ///
    /// `globalThis.File = class File extends Blob {}` binds `File` for the class
    /// body and nowhere else, so another module's `File` is still the host class.
    pub local_class_expressions: HashSet<smelt_hir::Symbol>,
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
    /// Module-level `const X = Symbol(..)` bindings, recorded crate-wide by
    /// [`crate::predeclare_unique_symbol_consts_with_path`] before any module
    /// body is lowered: declaring-module key (the module path, plus its
    /// canonical form) -> binding name -> the symbol's value spelling.
    ///
    /// A unique symbol bound once at module level is a static member name
    /// (`class C { get [K]() {..} }`, `c[K]`). An import cycle can lower a
    /// consumer of `K` before `K`'s own module, so the consumer could not
    /// otherwise know which member `c[K]` names; this table is what lets it.
    pub predeclared_unique_symbols: HashMap<String, HashMap<String, String>>,
}

impl HirCtx {
    /// Create a new empty HIR context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
            export_aliases: HashMap::new(),
            known_modules: HashSet::new(),
            module_exports: HashMap::new(),
            item_predicates: HashMap::new(),
            module_global_object_aliases: HashMap::new(),
            object_namespaces: Published::default(),
            object_consts: Published::default(),
            object_value_collections: Published::default(),
            const_collections: Published::default(),
            enum_members: Published::default(),
            overloads: Published::default(),
            function_rests: Published::default(),
            date_returning_functions: HashSet::new(),
            type_alias_fields: Published::default(),
            class_type_params: HashMap::new(),
            class_constructor_params: HashMap::new(),
            derived_records: HashMap::new(),
            erased_item_type_params: HashMap::new(),
            interface_extends: Published::default(),
            interface_index_values: Published::default(),
            class_index_values: Published::default(),
            interface_call_signatures: Published::default(),
            interface_construct_signatures: Published::default(),
            callable_fields: Published::default(),
            callable_object_aliases: Published::default(),
            written_host_globals: HashSet::new(),
            project_sources_outside_crate: HashSet::new(),
            type_renames: HashMap::new(),
            top_level_type_names: HashSet::new(),
            declared_member_names: HashSet::new(),
            local_class_expressions: HashSet::new(),
            module_identities: HashMap::new(),
            predeclared_unique_symbols: HashMap::new(),
        }
    }
}

impl Default for HirCtx {
    /// Create a new HIR context (same as `new`).
    fn default() -> Self {
        Self::new()
    }
}
