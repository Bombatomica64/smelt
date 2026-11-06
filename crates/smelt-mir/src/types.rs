//! Core MIR type definitions and structures.
//!
//! This module defines the fundamental types and structures used in the MIR representation,
//! including functions, basic blocks, locals, and various statements and expressions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use smelt_hir::{BodyId, Span, Symbol, TypeId, Visibility};

/// Unique identifier for a function in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FuncId(pub u32);

/// Unique identifier for a basic block in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

/// Unique identifier for a local variable in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

/// Unique identifier for a closure in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClosureId(pub u32);

/// A replacement argument passed to array splice-style MIR operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirListSpliceItem {
    /// Replacement operand or array operand when `spread` is true.
    pub value: Operand,
    /// Whether the source argument used JavaScript spread syntax.
    pub spread: bool,
}

/// The MIR representation of a crate, containing all functions, classes, and interfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mir {
    /// All functions in the crate.
    pub functions: Vec<MirFunction>,
    /// All classes in the crate.
    pub classes: Vec<MirClass>,
    /// All interfaces in the crate.
    pub interfaces: Vec<MirInterface>,
    /// All closure bodies in the crate.
    pub closures: Vec<MirClosure>,
    /// Module-level mutable globals lifted from source `let`/`var` bindings.
    ///
    /// Populated during HIR lowering from every [`smelt_hir::Item::MutableGlobal`]
    /// in the crate. `Rvalue::GlobalGet`/`Rvalue::GlobalSet` reference entries by
    /// index, and codegen emits one thread-local cell per entry.
    pub globals: Vec<MirGlobal>,
    /// Source path of every file the crate was lowered from, keyed by the
    /// [`smelt_hir::FileId`] its spans carry.
    ///
    /// MIR is flattened, so once lowering is done a diagnostic has a span but
    /// no way to say WHICH file it points into. Every `Span` carries a
    /// `FileId`, and the HIR crate's modules carry `FileId -> path`, so this
    /// preserves that one mapping across the boundary. It exists so an
    /// `EmitError` can name its site: before it, a codegen blocker in a closure
    /// could only report `<unnamed>`, and pinning one in a corpus the size of
    /// Hono took a manifest-level bisection.
    pub file_paths: HashMap<smelt_hir::FileId, String>,
    /// Type interner for interned types.
    pub types: smelt_hir::TypeInterner,
    /// Symbol interner for interned identifiers.
    pub symbols: smelt_hir::SymbolInterner,
    /// Original source spellings for symbols that were normalized internally.
    pub names: smelt_hir::OriginalNameTable,
    /// Record fields stored as accessors; see
    /// [`smelt_hir::Crate::accessor_fields`].
    #[serde(default)]
    pub accessor_fields: Vec<(Symbol, Symbol)>,
}

/// A module-level mutable global lowered from a source `let`/`var` binding.
///
/// Carries the binding's name, primitive type, and literal initializer. Codegen
/// mangles a per-program thread-local cell name from `name` and the global's
/// index so cross-module bindings that share a source name stay distinct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirGlobal {
    /// The binding's name symbol.
    pub name: Symbol,
    /// The binding's type.
    pub ty: TypeId,
    /// How the binding's initial value is produced.
    pub init: MirGlobalInit,
}

/// How a mutable global's cell is initialized in generated Rust.
///
/// A `thread_local!` initializer is an arbitrary Rust expression evaluated once
/// per thread on first access, so both forms below are legal there; the split
/// only records whether the value is a constant or has to be computed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirGlobalInit {
    /// A constant, emitted inline (and inside a `const` block where the type
    /// allows it).
    Constant(Constant),
    /// A call to the synthesized nullary initializer function the frontend
    /// built from the binding's initializer expression.
    Call(FuncId),
}

impl Mir {
    /// Creates a new empty MIR crate with the given type and symbol interners.
    #[must_use]
    pub fn new(
        types: smelt_hir::TypeInterner,
        symbols: smelt_hir::SymbolInterner,
        names: smelt_hir::OriginalNameTable,
    ) -> Self {
        Self {
            functions: Vec::new(),
            classes: Vec::new(),
            interfaces: Vec::new(),
            closures: Vec::new(),
            globals: Vec::new(),
            file_paths: HashMap::new(),
            types,
            symbols,
            names,
            accessor_fields: Vec::new(),
        }
    }

    /// Returns the next available function ID.
    ///
    /// The id saturates to `u32::MAX` if the count does not fit in `u32`.
    #[must_use]
    pub fn next_function_id(&self) -> FuncId {
        FuncId(len_to_u32(self.functions.len(), "MIR function count"))
    }

    /// Adds a function to the MIR and returns its ID.
    ///
    /// The id saturates to `u32::MAX` if the count does not fit in `u32`.
    pub fn push_function(&mut self, function: MirFunction) -> FuncId {
        let id = FuncId(len_to_u32(self.functions.len(), "MIR function count"));
        debug_assert_eq!(
            function.id, id,
            "MIR function IDs must be insertion ordered"
        );
        self.functions.push(function);
        id
    }

    /// Adds a closure to the MIR and returns its ID.
    ///
    /// The id saturates to `u32::MAX` if the count does not fit in `u32`.
    pub fn push_closure(&mut self, closure: MirClosure) -> ClosureId {
        let id = ClosureId(len_to_u32(self.closures.len(), "MIR closure count"));
        debug_assert_eq!(closure.id, id, "MIR closure IDs must be insertion ordered");
        self.closures.push(closure);
        id
    }
}

/// MIR representation of a closure body and explicit environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirClosure {
    /// Unique closure identifier.
    pub id: ClosureId,
    /// Parameter local IDs.
    pub params: Vec<LocalId>,
    /// Index of the packed rest parameter, if this closure came from `...args`.
    pub rest: Option<usize>,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    pub required_params: Option<usize>,
    /// All local variables in the closure body.
    pub locals: Vec<LocalDecl>,
    /// Captured environment entries.
    pub captures: Vec<MirClosureCapture>,
    /// Return type produced by the closure.
    pub return_ty: TypeId,
    /// Lowered blocks for the closure body.
    pub blocks: Vec<BasicBlock>,
    /// Entry block ID.
    pub entry: BlockId,
    /// Whether this closure escapes the creating function through a return.
    ///
    /// Escaping closures must own their captured environment in generated Rust.
    /// Non-escaping closures can borrow captures, which preserves source
    /// mutation semantics for iterator-style callbacks.
    pub escapes: bool,
    /// Whether this closure can return through a source-language throw.
    pub can_throw: bool,
    /// Whether invoking this closure constructs suspended generator state.
    pub is_generator: bool,
    /// Stable per-function-item key when this closure is a bare
    /// function-item-as-value wrapper, else `None`.
    ///
    /// Carried over from `ClosureExpr::function_item` (the source `ItemId`
    /// index). When such a wrapper is erased to `SmeltUnknown`, codegen routes
    /// it through a per-item compile-time accessor (`__smelt_fn_value_<key>()`)
    /// that lazily builds and caches one shared erased value, so all references
    /// to the same named function compare equal under JavaScript reference
    /// identity (`===`). The `ItemId` index is crate-unique, so it is a safe
    /// accessor key across all references.
    pub function_item_key: Option<usize>,
    /// How this closure body's source language spells an absent value.
    ///
    /// See `MirFunction::absent`. A closure body is emitted through its own
    /// synthesized `MirFunction`, so the spelling has to travel with it.
    pub absent: AbsentSpelling,
}

/// One explicit MIR closure capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirClosureCapture {
    /// Source local captured from the enclosing function.
    pub source_local: LocalId,
    /// Local inside the closure body that receives this captured value.
    pub target_local: Option<LocalId>,
    /// Capture symbol for diagnostics and codegen names.
    pub symbol: Symbol,
    /// Captured type.
    pub ty: TypeId,
    /// Capture mode.
    pub mode: smelt_hir::CaptureMode,
}

/// MIR representation of a class definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirClass {
    /// Name of the class.
    pub name: Symbol,
    /// Propagated from HIR for codegen (e.g. emit `#[derive(PartialEq, Eq)]`
    /// for frozen dataclasses).
    pub kind: smelt_hir::ClassKind,
    /// Generic type parameters declared by the class.
    pub type_params: Vec<smelt_hir::TypeParamDef>,
    /// Whether this class is abstract and therefore non-constructible.
    pub is_abstract: bool,
    /// Single base class, if any (multiple inheritance is rejected upstream).
    pub base: Option<Symbol>,
    /// Type arguments applied to the base class.
    pub base_args: Vec<TypeId>,
    /// Fields defined in the class.
    pub fields: Vec<MirField>,
    /// Class-level fields materialized at definition time.
    pub static_fields: Vec<MirStaticField>,
    /// Materialized typed descriptors.
    pub descriptors: Vec<MirDescriptor>,
    /// Constructor function ID, if any.
    pub constructor: Option<FuncId>,
    /// Method function IDs.
    pub methods: Vec<FuncId>,
    /// Static method function IDs (receiver-free associated functions).
    pub static_methods: Vec<FuncId>,
    /// Abstract method signatures required by this class.
    pub abstract_methods: Vec<smelt_hir::MethodSig>,
    /// Interfaces this class implements.
    pub implements: Vec<Symbol>,
    /// Source-language protocols with typed backend mappings.
    pub protocols: Vec<MirClassProtocol>,
}

/// A class protocol resolved to MIR function identities.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MirClassProtocol {
    /// Python `__add__` implemented by a concrete method.
    Add {
        /// MIR function implementing the protocol.
        method: FuncId,
    },
}

/// MIR representation of a materialized typed descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirDescriptor {
    /// Bound member name.
    pub name: Symbol,
    /// Type produced by reads.
    pub read_ty: TypeId,
    /// Type accepted by writes.
    pub write_ty: Option<TypeId>,
    /// Getter function, when source-mappable.
    pub getter: Option<FuncId>,
    /// Setter function, when source-mappable.
    pub setter: Option<FuncId>,
    /// Whether data-descriptor precedence applies.
    pub data_descriptor: bool,
    /// Whether the descriptor is bound on the constructor.
    pub is_static: bool,
    /// Concrete descriptor instance state.
    pub value_fields: Vec<smelt_hir::DescriptorValueField>,
}

/// MIR representation of an interface definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirInterface {
    /// Name of the interface.
    pub name: Symbol,
    /// Generic type parameters declared by the interface.
    pub type_params: Vec<smelt_hir::TypeParamDef>,
    /// Interfaces extended by this interface.
    pub extends: Vec<smelt_hir::InterfaceHeritage>,
    /// Fields defined in the interface.
    pub fields: Vec<MirField>,
    /// Method signatures in the interface.
    pub methods: Vec<smelt_hir::MethodSig>,
    /// Per-property facts for strict JSON entry decoding; see
    /// [`smelt_hir::FieldDecodeFacts`].
    #[serde(default)]
    pub decode_facts: Vec<smelt_hir::FieldDecodeFacts>,
}

/// MIR representation of a field in a class or interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirField {
    /// Name of the field.
    pub name: Symbol,
    /// Type of the field.
    pub ty: TypeId,
    /// Visibility of the field.
    pub visibility: Visibility,
}

/// MIR representation of a materialized class-level field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirStaticField {
    /// Member name.
    pub name: Symbol,
    /// Concrete field type.
    pub ty: TypeId,
    /// Source visibility.
    pub visibility: Visibility,
    /// Materialized primitive value.
    pub value: Option<smelt_hir::Literal>,
}

/// MIR representation of a function with basic blocks and locals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "orthogonal source function flags are preserved independently through MIR"
)]
pub struct MirFunction {
    /// Unique identifier of this function.
    pub id: FuncId,
    /// Name of the function.
    pub name: Symbol,
    /// Generic type parameters declared by a generic free function.
    ///
    /// Propagated from HIR so codegen can emit real Rust generics
    /// (`fn identity<T>(x: T) -> T`) for a generic free function and keep `T`
    /// in scope while rendering its parameter and return types. Class members
    /// carry no entries here; their generics come from the owning `MirClass`.
    pub type_params: Vec<smelt_hir::TypeParamDef>,
    /// Origin information (from HIR).
    pub origin: HirOrigin,
    /// Whether this is an async function.
    pub is_async: bool,
    /// Whether invoking this function constructs suspended generator state.
    pub is_generator: bool,
    /// Whether this function should be emitted as a native Rust test.
    pub is_test: bool,
    /// Whether this function can return through an uncaught throw path.
    pub can_throw: bool,
    /// Parameter local IDs.
    pub params: Vec<LocalId>,
    /// Index of the packed rest parameter, if this function came from `...args`.
    pub rest: Option<usize>,
    /// Return type of the function.
    pub return_ty: TypeId,
    /// All local variables in the function.
    pub locals: Vec<LocalDecl>,
    /// All basic blocks in the function.
    pub blocks: Vec<BasicBlock>,
    /// Entry block ID.
    pub entry: BlockId,
    /// How this body's SOURCE LANGUAGE spells an absent value.
    ///
    /// Same rule, and the same reason, as `BuiltinFn::ConsoleLog`'s own field:
    /// stringifying an `Optional` that holds nothing is `undefined` in
    /// JavaScript and `None` in Python, and the disagreement is not observable
    /// from the operand or its type. It is carried per BODY rather than per
    /// node because a body comes from exactly one source file while the sites
    /// that need the answer -- `String(x)`, a template interpolation, a `+`
    /// concatenation -- are ordinary rvalues that no one would widen for it.
    pub absent: AbsentSpelling,
}

impl MirFunction {
    /// Creates a new MIR function with an empty entry block.
    pub(crate) fn new(
        id: FuncId,
        name: Symbol,
        origin: HirOrigin,
        return_ty: TypeId,
        span: Span,
    ) -> Self {
        Self {
            id,
            name,
            type_params: Vec::new(),
            origin,
            is_async: false,
            is_generator: false,
            is_test: false,
            can_throw: false,
            params: Vec::new(),
            rest: None,
            return_ty,
            locals: Vec::new(),
            blocks: vec![BasicBlock {
                id: BlockId(0),
                statements: Vec::new(),
                terminator: None,
                span,
            }],
            entry: BlockId(0),
            // Overwritten by lowering from the body's own file; the default is
            // the TypeScript-shaped answer, matching `absent_spelling`'s own
            // fallback for a synthesized span with no source module.
            absent: AbsentSpelling::Undefined,
        }
    }

    /// Adds a local variable to the function and returns its ID.
    ///
    /// The id saturates to `u32::MAX` if the count does not fit in `u32`.
    pub(crate) fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(len_to_u32(self.locals.len(), "MIR local count"));
        self.locals.push(local);
        id
    }

    /// Adds a basic block to the function and returns its ID.
    ///
    /// The id saturates to `u32::MAX` if the count does not fit in `u32`.
    pub(crate) fn push_block(&mut self, span: Span) -> BlockId {
        let id = BlockId(len_to_u32(self.blocks.len(), "MIR block count"));
        self.blocks.push(BasicBlock {
            id,
            statements: Vec::new(),
            terminator: None,
            span,
        });
        id
    }
}

/// Convert a length into a `u32` identifier.
fn len_to_u32(len: usize, label: &str) -> u32 {
    u32::try_from(len).unwrap_or_else(|_| {
        let _ = label;
        u32::MAX
    })
}

/// Information about the origin of a MIR function in the HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirOrigin {
    /// A module-level function body.
    Body(BodyId),
    /// A class constructor.
    ClassConstructor {
        /// The class being constructed.
        class: Symbol,
        /// The constructor body.
        body: BodyId,
    },
    /// A class method.
    ClassMethod {
        /// The class containing the method.
        class: Symbol,
        /// The method name.
        method: Symbol,
        /// The method body.
        body: BodyId,
    },
    /// A `static` class method (associated function, no receiver).
    ClassStaticMethod {
        /// The class containing the static method.
        class: Symbol,
        /// The method name.
        method: Symbol,
        /// The method body.
        body: BodyId,
    },
}

/// Declaration of a local variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDecl {
    /// Type of the local.
    pub ty: TypeId,
    /// Kind of the local (parameter, temporary, or user binding).
    pub kind: LocalKind,
    /// Source span of the local.
    pub span: Span,
}

/// The kind of a local variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalKind {
    /// A function parameter.
    Param {
        /// Source symbol when this parameter comes from a named source-level declaration.
        ///
        /// Synthetic parameters (for example callback bridge slots) may not have
        /// a user-authored name and keep this value as `None`.
        symbol: Option<Symbol>,
    },
    /// A compiler-generated temporary.
    Temp,
    /// A user-defined binding (variable).
    UserBinding(Symbol),
}

/// A basic block in MIR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    /// The block identifier.
    pub id: BlockId,
    /// Statements executed in order.
    pub statements: Vec<Statement>,
    /// The block terminator, if present.
    pub terminator: Option<Terminator>,
    /// Source span associated with the block.
    pub span: Span,
}

/// A memory location used by MIR operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Place {
    /// A local variable.
    Local(LocalId),
    /// A field on a local variable.
    Field {
        /// The base local.
        base: LocalId,
        /// The field name.
        field: Symbol,
    },
    /// An indexed lookup on a local variable.
    Index {
        /// The base local.
        base: LocalId,
        /// The index expression.
        index: Box<Operand>,
        /// What a negative index means at this site.
        negative: NegativeIndex,
    },
    /// A field or index projection rooted at a module-level mutable global.
    ///
    /// Distinct from the local-rooted variants because the base is not a local
    /// at all: it is a `thread_local!` cell, and the whole point of the variant
    /// is to mutate the value INSIDE that cell rather than a copy read out of
    /// it. A `GlobalGet` yields a clone, which shares the store for a handle
    /// type (`SmeltRecord`) and deep-copies for a value type (`HashMap`), so a
    /// write through a materialized copy is correct for one and silently lost
    /// for the other. Naming the cell as the assignment root avoids having to
    /// answer which case applies. See
    /// `blocker-logs/hono-h6-place-global.md`.
    ///
    /// Whole-binding assignment is NOT this variant: `x = e` lowers to
    /// `ExprKind::GlobalSet`, which replaces the cell's value and needs no
    /// projection.
    Global {
        /// Index into [`Mir::globals`].
        base: u32,
        /// What is written inside the cell.
        projection: GlobalProjection,
    },
}

/// What a [`Place::Global`] writes inside a mutable global's cell.
///
/// One level only. A nested write (`cache[a][b] = v`) is not this shape: the
/// inner `cache[a]` has to produce a value, and whether that value shares with
/// the cell is the same handle-versus-value question `Place::Global` exists to
/// avoid asking. Nested writes keep their frontend blocker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GlobalProjection {
    /// `global.field = value`.
    Field(Symbol),
    /// `global[index] = value`.
    Index {
        /// The index expression.
        index: Box<Operand>,
        /// What a negative index means at this site.
        negative: NegativeIndex,
    },
}

/// How a source language spells a value that is absent.
///
/// Only reached when printing an `Optional<T>` that holds nothing. A Rust
/// `Option` has no place in program output -- `{:?}` on one printed
/// `Some("ada")` / `None`, a shape no source language produces -- so the
/// present arm prints the value inside and the absent arm prints this word.
///
/// TypeScript's `null` and `undefined` both intern to `Type::None`, so
/// `T | null` and `T | undefined` are one type by the time this is chosen, and
/// `Undefined` is the answer for both: it is what nearly every operation that
/// *produces* an optional in TypeScript returns (`find`, `pop`, `Map.get`, an
/// optional property or parameter, `?.`, `process.env.X`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbsentSpelling {
    /// JavaScript/TypeScript: `undefined`.
    Undefined,
    /// Python: `None`.
    None,
}

impl AbsentSpelling {
    /// The word this language prints for an absent value.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Undefined => "undefined",
            Self::None => "None",
        }
    }

    /// The word this language prints for an explicit NULL value.
    ///
    /// JavaScript distinguishes the two — `String(null)` is `"null"` while
    /// `String(undefined)` is `"undefined"` — and Python has only one absent
    /// value, so both words are `None` there. The distinction is observable
    /// wherever the value still carries its runtime tag
    /// (`SmeltUnknown::Null` vs `SmeltUnknown::Undefined`); a typed `Optional`
    /// has erased it and uses [`Self::text`], which is the common case in
    /// TypeScript (`find`, `Map.get`, an optional property, `?.`).
    #[must_use]
    pub const fn null_text(self) -> &'static str {
        match self {
            Self::Undefined => "null",
            Self::None => "None",
        }
    }
}

/// What a negative element index means at an indexed place.
///
/// JavaScript and Python disagree, and the disagreement is not observable from
/// the index value or the collection type: `xs[-1]` is `undefined` in
/// JavaScript but the LAST ELEMENT in Python. The rule therefore belongs to the
/// source language of the site, which only HIR lowering knows -- a single crate
/// can mix TypeScript and Python modules, so codegen cannot infer it and must
/// not guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NegativeIndex {
    /// Python: a negative index counts back from the collection's end, so it is
    /// normalized to `len + index` before the slot is addressed.
    FromEnd,
    /// JavaScript: a negative subscript addresses no element at all. It is a
    /// property key, not a position, so an element read answers `undefined`
    /// rather than wrapping around to a real slot.
    OutOfRange,
}

/// An operand in MIR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operand {
    /// Copy from a place.
    Copy(Place),
    /// Move from a place.
    Move(Place),
    /// Constant literal.
    Const(Constant),
}

/// A literal constant value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    /// Boolean constant.
    Bool(bool),
    /// Integer constant.
    Int(i64),
    /// Floating-point constant.
    Float(f64),
    /// JavaScript bigint constant, as canonical decimal digits (see
    /// `smelt_hir::Literal::BigInt`).
    BigInt(String),
    /// String constant.
    String(String),
    /// JavaScript symbol constant.
    Symbol(String),
    /// JavaScript `undefined` constant.
    Undefined,
    /// `None`.
    None,
}

/// Generates [`Rvalue`]: the forms only MIR has, then every operation in the op
/// table.
macro_rules! define_rvalue {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        /// An MIR rvalue.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub enum Rvalue {
            /// Use an existing operand directly.
            Use(Operand),
            /// Suspend the current generator and expose a value to its caller.
            GeneratorYield {
                /// Value exposed by the suspension point.
                value: Operand,
                /// Lexical catch continuation active at the suspension point.
                unwind: Option<ExceptionHandler>,
                /// Innermost finally continuation active at the suspension point.
                cleanup: Option<GeneratorCleanup>,
            },
            /// Construct a list.
            List(Vec<Operand>),
            /// Construct a set.
            Set(Vec<Operand>),
            /// Construct a dictionary.
            Dict(Vec<(Operand, Operand)>),
            /// Construct a tuple.
            Tuple(Vec<Operand>),
            /// Apply a binary operator.
            Binary {
                /// The operator.
                op: smelt_hir::BinOp,
                /// Left-hand operand.
                lhs: Operand,
                /// Right-hand operand.
                rhs: Operand,
            },
            /// Apply a unary operator.
            Unary {
                /// The operator.
                op: smelt_hir::UnaryOp,
                /// The operand.
                operand: Operand,
            },
            /// Call a method shared by every concrete arm of a tagged union.
            UnionMethod {
                /// Tagged-union receiver whose active arm selects the implementation.
                receiver: Operand,
                /// Shared source method name.
                method: Symbol,
                /// Call arguments evaluated before dispatch.
                args: Vec<Operand>,
            },
            /// Call a method through a type parameter's class or interface bound
            /// (`a.speak()` with `a: A` and `A extends Animal`).
            ///
            /// The call is typed by the bound's declaration but dispatched by the
            /// receiver's concrete type, so it has no single static callee: codegen
            /// renders it through the generated bound trait (`A: AnimalLike`) when the
            /// parameter is a real Rust generic, and as a dynamic member call when the
            /// parameter erased.
            BoundMethod {
                /// Receiver whose static type is the bounded type parameter.
                receiver: Operand,
                /// The bound's class or interface name.
                bound: Symbol,
                /// Source method name.
                method: Symbol,
                /// Call arguments, receiver excluded.
                args: Vec<Operand>,
            },
            /// Read a property through a type parameter's class or interface bound
            /// (`a.legs` with `a: A` and `A extends Animal`); see [`Self::BoundMethod`].
            BoundField {
                /// Receiver whose static type is the bounded type parameter.
                receiver: Operand,
                /// The bound's class or interface name.
                bound: Symbol,
                /// Source property name.
                field: Symbol,
            },
            /// Construct a class instance.
            Struct {
                /// The class being constructed.
                class: Symbol,
                /// Field initializers.
                fields: Vec<(Symbol, Operand)>,
            },
            /// Construct an imported class value whose implementation is not part of this crate.
            ExternalClassInstance {
                /// The imported class being constructed.
                class: Symbol,
                /// Constructor arguments, kept so their expressions are lowered before erasure.
                args: Vec<Operand>,
            },
            /// Construct a closure value from a MIR closure body and captured operands.
            Closure {
                /// Closure table identifier.
                id: ClosureId,
                /// Captured operands in closure capture order.
                captures: Vec<Operand>,
            },
            /// Read a module-level mutable global by index into `Mir::globals`.
            GlobalGet {
                /// Index into `Mir::globals`.
                global: u32,
            },
            /// Store into a module-level mutable global; evaluates to the stored value.
            GlobalSet {
                /// Index into `Mir::globals`.
                global: u32,
                /// The value to store.
                value: Operand,
            },
            /// Await a future and produce its output.
            Await(Operand),
            $(
                $(#[$meta])*
                $name $({
                    $(
                        $(#[$field_meta])*
                        $field: smelt_hir::op_field_ty!(Operand, MirListSpliceItem, $kind),
                    )*
                })?,
            )*
        }
    };
}

smelt_hir::for_each_op!(define_rvalue);

/// A MIR statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    /// Assign to a local.
    Assign {
        /// The destination local.
        dest: LocalId,
        /// The assigned value.
        value: Rvalue,
    },
    /// Assign to a place.
    AssignPlace {
        /// The destination place.
        place: Place,
        /// The assigned value.
        value: Rvalue,
    },
    /// Read-modify-write of one dictionary entry through a single container probe.
    ///
    /// Produced only by [`crate::opt::DictEntryUpdate`], which fuses the
    /// read/compute/write-back triple a source `d[k] = f(d[k] ?? seed)` lowers
    /// to. The statement means, in order:
    ///
    /// 1. locate the entry `base[index]`, inserting `default` when `index` is
    ///    absent;
    /// 2. bind the entry's value (existing or freshly seeded) to `current`;
    /// 3. evaluate `value` and store it back into the same entry.
    ///
    /// `index` and `default` are evaluated BEFORE the entry is borrowed;
    /// `value` is evaluated while it is held, so the pass that forms this
    /// statement proves `value` cannot reach the container (see that module's
    /// correctness conditions).
    DictEntryUpdate {
        /// Local holding the dictionary. A local rather than a [`Place`]
        /// because the backend's entry accessors (`SmeltJsMap`/`SmeltRecord`
        /// `entry_or_insert`, `HashMap::entry`) are only reachable through a
        /// directly named container, exactly as in [`Rvalue::ListPush`].
        base: LocalId,
        /// Key selecting the entry, evaluated once instead of twice.
        index: Operand,
        /// Value seeded into an absent entry before it is read. An operand, not
        /// an [`Rvalue`], because the backend passes it to the entry accessor
        /// as a closure that may run under the container's own borrow.
        default: Operand,
        /// Local bound to the entry's value for the duration of `value`. A
        /// declared MIR local rather than a fresh binding so it keeps its type
        /// and so the shared local read/write accounting still sees the write.
        current: LocalId,
        /// New value stored back into the entry. An [`Rvalue`], not an
        /// [`Operand`], because the modify step is the whole point: it runs
        /// inside the single probe and may read `current`.
        value: Rvalue,
    },
}

/// A MIR terminator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Terminator {
    /// Jump to another block.
    Goto(BlockId),
    /// Call a callee and continue in the target block.
    Call {
        /// The callee to invoke.
        callee: Callee,
        /// Call arguments.
        args: Vec<Operand>,
        /// Destination for the result.
        dest: LocalId,
        /// Successor block after the call.
        target: BlockId,
        /// Exception handler used when a throwing callee returns an error.
        unwind: Option<ExceptionHandler>,
    },
    /// Await a future and continue in the target block.
    Await {
        /// Future operand to await.
        future: Operand,
        /// Destination for the resolved value.
        dest: LocalId,
        /// Successor block after the await resolves.
        target: BlockId,
        /// Exception handler used when an awaited rejecting future returns an error.
        unwind: Option<ExceptionHandler>,
    },
    /// Branch on a boolean condition.
    Switch {
        /// The condition value.
        cond: Operand,
        /// Block taken when the condition is true.
        then_block: BlockId,
        /// Block taken when the condition is false.
        else_block: BlockId,
    },
    /// Branch on constant labels.
    Match {
        /// The value being matched.
        scrutinee: Operand,
        /// Match arms.
        arms: Vec<MatchArm>,
        /// Default block when no arm matches.
        default: Option<BlockId>,
    },
    /// Return from the function.
    Return(Operand),
    /// Raise an exception-like value and stop normal control flow.
    Throw(Operand),
}

impl Terminator {
    /// Return the control-flow successor blocks of this terminator.
    ///
    /// Exception handlers count as successors so dataflow analyses see the
    /// throwing edge of a `Call`/`Await`. `Return` and `Throw` have none.
    #[must_use]
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Self::Goto(target) => vec![*target],
            Self::Call { target, unwind, .. } | Self::Await { target, unwind, .. } => unwind
                .iter()
                .map(|handler| handler.catch_block)
                .chain(std::iter::once(*target))
                .collect(),
            Self::Switch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            Self::Match { arms, default, .. } => {
                arms.iter().map(|arm| arm.target).chain(*default).collect()
            }
            Self::Return(_) | Self::Throw(_) => Vec::new(),
        }
    }
}

/// MIR edge for source-language exceptions from throwing calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionHandler {
    /// Block reached when the call throws.
    pub catch_block: BlockId,
    /// Optional local receiving the thrown payload.
    pub exception_local: Option<LocalId>,
}

/// MIR edge used when an abrupt generator command must execute `finally` first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorCleanup {
    /// Entry block of the active finally clause.
    pub block: BlockId,
    /// Block reached after the finally clause completes normally.
    pub after: BlockId,
}

/// A single match arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    /// The matched label.
    pub label: Constant,
    /// The target block.
    pub target: BlockId,
}

/// A callable target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Callee {
    /// A statically known function.
    Static(FuncId),
    /// An indirect call through a runtime value.
    Indirect(Operand),
    /// A builtin function.
    Builtin(BuiltinFn),
    /// An operation that can throw (see [`Rvalue::is_fallible`]), carried here
    /// for the call terminator's unwind edge. Its operands are its own fields;
    /// the call's `args` are empty.
    Op(Box<Rvalue>),
    /// A base class's constructor run as an initializer over a derived instance
    /// (a derived constructor's `super(..)`; see
    /// `smelt_hir::ExprKind::BaseConstructorInit`).
    ///
    /// The call's first argument is the receiver (the derived `this`), the rest
    /// are the base constructor's arguments; the call evaluates to the
    /// initialised receiver. The `FuncId` names the BASE constructor, whose
    /// body codegen re-emits over the derived `Self`.
    BaseInit(FuncId),
}

/// Builtin functions recognized by MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltinFn {
    /// Print to the console.
    ///
    /// Carries how the SOURCE LANGUAGE spells an absent value, because both
    /// frontends lower to this one builtin and they disagree: printing a
    /// `string | undefined` that holds nothing is `undefined` in JavaScript and
    /// `None` in Python. The disagreement is not observable from the operand or
    /// its type, so -- exactly like [`NegativeIndex`] -- the rule belongs to the
    /// site's language, which only lowering still knows.
    ConsoleLog {
        /// How this call site's language spells an absent value.
        absent: AbsentSpelling,
    },
    /// Write exact text to stdout.
    ConsoleWrite,
    /// Write exact text to stderr.
    ConsoleErrorWrite,
}

/// Visit every type id a function's emission reads: its return type, local
/// types, and `UnknownCast` targets (the only statement that carries a
/// type).
pub fn for_each_function_type(function: &MirFunction, mut visit: impl FnMut(TypeId)) {
    visit(function.return_ty);
    for local in &function.locals {
        visit(local.ty);
    }
    for block in &function.blocks {
        for statement in &block.statements {
            if let Statement::Assign {
                value: Rvalue::UnknownCast { target, .. },
                ..
            } = statement
            {
                visit(*target);
            }
        }
    }
}

/// Rewrite every type id [`for_each_function_type`] visits.
pub fn map_function_types(function: &mut MirFunction, mut map: impl FnMut(TypeId) -> TypeId) {
    function.return_ty = map(function.return_ty);
    for local in &mut function.locals {
        local.ty = map(local.ty);
    }
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            if let Statement::Assign {
                value: Rvalue::UnknownCast { target, .. },
                ..
            } = statement
            {
                *target = map(*target);
            }
        }
    }
}
