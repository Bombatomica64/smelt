//! Core MIR type definitions and structures.
//!
//! This module defines the fundamental types and structures used in the MIR representation,
//! including functions, basic blocks, locals, and various statements and expressions.

use serde::{Deserialize, Serialize};
use smelt_hir::{BodyId, PropertyLookup, Span, Symbol, TypeId, Visibility};

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
    /// Type interner for interned types.
    pub types: smelt_hir::TypeInterner,
    /// Symbol interner for interned identifiers.
    pub symbols: smelt_hir::SymbolInterner,
    /// Original source spellings for symbols that were normalized internally.
    pub names: smelt_hir::OriginalNameTable,
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
    /// The binding's primitive type (Float, Int, Bool, or String in V1).
    pub ty: TypeId,
    /// The binding's literal initializer.
    pub init: Constant,
}

impl Mir {
    /// Creates a new empty MIR crate with the given type and symbol interners.
    #[must_use]
    pub const fn new(
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
            types,
            symbols,
            names,
        }
    }

    /// Returns the next available function ID.
    ///
    /// # Panics
    ///
    /// Panics if the number of functions does not fit in `u32`.
    #[must_use]
    pub fn next_function_id(&self) -> FuncId {
        FuncId(len_to_u32(self.functions.len(), "MIR function count"))
    }

    /// Adds a function to the MIR and returns its ID.
    ///
    /// # Panics
    ///
    /// Panics if the number of functions does not fit in `u32`.
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
    /// # Panics
    ///
    /// Panics if the number of closures does not fit in `u32`.
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
                phis: Vec::new(),
                statements: Vec::new(),
                terminator: None,
                span,
            }],
            entry: BlockId(0),
        }
    }

    /// Adds a local variable to the function and returns its ID.
    ///
    /// # Panics
    ///
    /// Panics if the number of locals does not fit in `u32`.
    pub(crate) fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(len_to_u32(self.locals.len(), "MIR local count"));
        self.locals.push(local);
        id
    }

    /// Adds a basic block to the function and returns its ID.
    ///
    /// # Panics
    ///
    /// Panics if the number of basic blocks does not fit in `u32`.
    pub(crate) fn push_block(&mut self, span: Span) -> BlockId {
        let id = BlockId(len_to_u32(self.blocks.len(), "MIR block count"));
        self.blocks.push(BasicBlock {
            id,
            phis: Vec::new(),
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
    /// Phi nodes at the top of the block.
    pub phis: Vec<Phi>,
    /// Statements executed in order.
    pub statements: Vec<Statement>,
    /// The block terminator, if present.
    pub terminator: Option<Terminator>,
    /// Source span associated with the block.
    pub span: Span,
}

/// A phi node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phi {
    /// Destination local for the phi.
    pub dest: LocalId,
    /// Type of the phi result.
    pub ty: TypeId,
    /// Incoming values by predecessor block.
    pub incoming: Vec<(BlockId, Operand)>,
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
    /// String constant.
    String(String),
    /// JavaScript symbol constant.
    Symbol(String),
    /// JavaScript `undefined` constant.
    Undefined,
    /// `None`.
    None,
}

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
    /// Resume a synchronous generator once.
    GeneratorNext {
        /// Generator handle to resume.
        generator: Operand,
        /// Optional value sent into the suspended yield expression.
        value: Option<Operand>,
        /// Protocol method used for this resumption.
        kind: GeneratorResumeKind,
    },
    /// Test whether a generator resume result represents completion.
    GeneratorDone {
        /// Resume result to inspect.
        result: Operand,
    },
    /// Extract the yielded or returned value from a generator resume result.
    GeneratorValue {
        /// Resume result whose payload is extracted.
        result: Operand,
    },
    /// Resume a delegated generator and forward its yielded values.
    GeneratorDelegate {
        /// Generator whose suspension points are forwarded.
        generator: Operand,
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
    /// Choose one of two operands based on a boolean condition.
    Conditional {
        /// Boolean condition to test.
        cond: Operand,
        /// Operand used when the condition is true.
        then_operand: Operand,
        /// Operand used when the condition is false.
        else_operand: Operand,
    },
    /// Select a first-class function value from a static string-keyed table.
    FunctionTableLookup {
        /// String key used to select the function.
        key: Operand,
        /// Function values keyed by source object field text.
        cases: Vec<(String, Operand)>,
    },
    /// Read a field through a TypeScript optional-chain receiver.
    OptionalField {
        /// Receiver operand, either `T` or `Option<T>`.
        receiver: Operand,
        /// Field name to read when the receiver is present.
        field: Symbol,
    },
    /// Read an index through a TypeScript optional-chain receiver.
    OptionalIndex {
        /// Receiver operand, either an indexable `T` or `Option<T>`.
        receiver: Operand,
        /// Index operand used when the receiver is present.
        index: Operand,
    },
    /// Call a method through a TypeScript optional-chain receiver.
    OptionalMethod {
        /// Receiver operand, either `T` or `Option<T>`.
        receiver: Operand,
        /// Method name to call when the receiver is present.
        method: Symbol,
        /// Call arguments.
        args: Vec<Operand>,
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
    /// Return an optional value's content or a fallback operand.
    OptionalCoalesce {
        /// Optional operand to inspect.
        optional: Operand,
        /// Fallback operand used when the optional is empty.
        fallback: Operand,
    },
    /// Test whether a value is an instance of a class.
    InstanceOf {
        /// Value being tested. Kept as an operand so side effects are evaluated before the check.
        value: Operand,
        /// Target class symbol.
        class: Symbol,
    },
    /// Test whether a value's prototype chain reaches a runtime constructor's
    /// `prototype` (`OrdinaryHasInstance`, JS `value instanceof target`).
    InstanceOfValue {
        /// Value whose prototype chain is walked.
        value: Operand,
        /// Constructor function value whose `prototype` is looked for.
        target: Operand,
    },
    /// Test the runtime tag of a TypeScript `unknown` value.
    UnknownIs {
        /// Value being tested.
        value: Operand,
        /// Runtime tag to check.
        kind: smelt_hir::UnknownKind,
    },
    /// Return the JavaScript `typeof` string for a runtime-erased value.
    TypeofValue {
        /// Value being classified.
        value: Operand,
    },
    /// Return the opaque `Object.getPrototypeOf` sentinel for an erased value.
    PrototypeSentinel {
        /// Value whose prototype sentinel is being computed.
        value: Operand,
        /// Whether an own `__proto__` slot on the receiver shadows the answer;
        /// see [`smelt_hir::ExprKind::PrototypeSentinel`].
        own_slot_shadows: bool,
    },
    /// Box a primitive the way `Object(value)` does; objects pass through
    /// (see the runtime `smelt_box_value`).
    BoxPrimitive {
        /// Value being boxed.
        value: Operand,
    },
    /// Create a fresh erased object from a runtime prototype value
    /// (`Object.create(proto)`; see the runtime `smelt_object_from_prototype`).
    ObjectFromPrototype {
        /// Prototype the fresh object inherits from.
        prototype: Operand,
    },
    /// Install a property-descriptor table onto an erased object
    /// (`Object.defineProperty` / `Object.defineProperties`; see the runtime
    /// `smelt_define_properties`).
    DefineProperties {
        /// Object the properties are installed on; also the result value.
        target: Operand,
        /// Erased object mapping property key to property descriptor.
        descriptors: Operand,
    },
    /// Extract a typed value from a TypeScript `unknown` value.
    UnknownCast {
        /// Value being extracted.
        value: Operand,
        /// Type expected after extraction.
        target: TypeId,
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
    /// Compute the length of a value.
    Len(Operand),
    /// Compute the absolute value of an integer or floating-point number.
    NumericAbs(Operand),
    /// Round a floating-point value with a standard numeric operation.
    NumericRound {
        /// Operation to apply.
        op: smelt_hir::NumericRoundOp,
        /// Numeric operand to round.
        operand: Operand,
    },
    /// Compute the minimum or maximum of floating-point operands.
    NumericExtrema {
        /// Operation to apply.
        op: smelt_hir::NumericExtremaOp,
        /// Numeric operands to compare.
        args: Vec<Operand>,
        /// Optional numeric list folded alongside `args`, produced when the
        /// source spreads a list into `Math.max`/`Math.min`.
        spread: Option<Operand>,
    },
    /// Compute the Euclidean norm of floating-point operands.
    NumericHypot {
        /// Numeric operands to combine.
        args: Vec<Operand>,
    },
    /// Test a numeric value with a numeric predicate.
    NumericPredicate {
        /// Predicate to apply.
        op: smelt_hir::NumericPredicateOp,
        /// Numeric operand to test.
        operand: Operand,
    },
    /// Apply a direct unary numeric function.
    NumericUnaryFunc {
        /// Operation to apply.
        op: smelt_hir::NumericUnaryFuncOp,
        /// Numeric operand to transform.
        operand: Operand,
    },
    /// Construct a closure value from a MIR closure body and captured operands.
    Closure {
        /// Closure table identifier.
        id: ClosureId,
        /// Captured operands in closure capture order.
        captures: Vec<Operand>,
    },
    /// Read the JavaScript `this` receiver installed by the innermost active call.
    ///
    /// Evaluates to the receiver a [`Rvalue::BindThis`] callable installed for
    /// the duration of the current invocation, or `undefined` when the current
    /// call supplied none (a plain `f()` invocation).
    ThisRead,
    /// Bind a receiver to a callable value, as `Function.prototype.bind` does.
    ///
    /// The result is a callable that installs `receiver` as the `this` seen by
    /// [`Rvalue::ThisRead`] inside the callee for the duration of one call and
    /// restores the previous binding afterwards.
    BindThis {
        /// The callable whose receiver is bound.
        callee: Operand,
        /// The receiver value the callee's `this` resolves to.
        receiver: Operand,
    },
    /// Call a closure value.
    ClosureCall {
        /// Closure value to call.
        callee: Operand,
        /// Call arguments.
        args: Vec<Operand>,
    },
    /// JavaScript `new callee(args)` through a function VALUE (`[[Construct]]`).
    ///
    /// Distinct from [`Rvalue::ClosureCall`] because construction allocates an
    /// object linked to the callee's `prototype`, runs the callee with that
    /// object as its receiver, and keeps the allocated object unless the callee
    /// returned one of its own.
    Construct {
        /// Function value being constructed through.
        callee: Operand,
        /// Constructor arguments.
        args: Vec<Operand>,
    },
    /// Call a closure value with a runtime argument vector from spread syntax.
    ClosureCallSpread {
        /// Closure value to call.
        callee: Operand,
        /// Runtime argument list.
        args: Operand,
    },
    /// Raise a floating-point base to a floating-point exponent.
    NumericPow {
        /// Base operand.
        base: Operand,
        /// Exponent operand.
        exponent: Operand,
    },
    /// Compute the arctangent of two numeric coordinates.
    NumericAtan2 {
        /// Y coordinate.
        y: Operand,
        /// X coordinate.
        x: Operand,
    },
    /// Generate a pseudo-random floating-point value in the half-open range `[0, 1)`.
    NumericRandom,
    /// Generate a pseudo-random integer in an inclusive range.
    NumericRandomInt {
        /// Inclusive lower bound.
        start: Operand,
        /// Inclusive upper bound.
        end: Operand,
    },
    /// Convert a numeric value to a string with a numeric radix.
    NumericToStringRadix {
        /// Numeric operand.
        operand: Operand,
        /// Numeric radix operand.
        radix: Operand,
    },
    /// Format a numeric value as a fixed-point decimal string
    /// (`Number.prototype.toFixed`).
    NumericToFixed {
        /// Numeric operand being formatted.
        operand: Operand,
        /// Number of fractional digits to render.
        digits: Operand,
    },
    /// Parse an integer from a string with a numeric radix.
    ParseIntRadix {
        /// String operand.
        operand: Operand,
        /// Numeric radix operand.
        radix: Operand,
    },
    /// Convert a primitive value to another primitive type.
    PrimitiveCast {
        /// Conversion operation to apply.
        op: smelt_hir::PrimitiveCastOp,
        /// Primitive operand to convert.
        operand: Operand,
    },
    /// Change the case of a string value.
    StringCase {
        /// Operation to apply to the string.
        op: smelt_hir::StringCaseOp,
        /// String operand to transform.
        operand: Operand,
    },
    /// Normalize a Unicode string value.
    StringNormalize {
        /// Normalization form to apply.
        form: smelt_hir::StringNormalizeForm,
        /// String operand to normalize.
        operand: Operand,
    },
    /// Collate two strings per JavaScript `left.localeCompare(right)` with no
    /// locale or option arguments (see the runtime `smelt_locale_compare`
    /// helper for the collation levels that are and are not modeled).
    StringLocaleCompare {
        /// Receiver string.
        left: Operand,
        /// String compared against the receiver.
        right: Operand,
    },
    /// Percent-encode a string per JavaScript `encodeURI` (the full-URI
    /// character set; see the runtime `smelt_encode_uri` helper).
    UriEncode {
        /// String operand to encode.
        operand: Operand,
    },
    /// Resolve the JavaScript `Object.prototype.toString.call(x)` tag
    /// (`"[object Tag]"`) from an erased value's runtime variant and host
    /// identity markers (see the runtime `smelt_object_to_string_tag` helper).
    ObjectToStringTag {
        /// Erased value operand to probe.
        operand: Operand,
    },
    /// Deep-copy an erased value graph with fresh identities, preserving host
    /// markers (`structuredClone(x)`; see the runtime `smelt_structured_clone`).
    StructuredClone {
        /// Erased value operand to deep-clone.
        operand: Operand,
    },
    /// Trim whitespace from a string value.
    StringTrim {
        /// Which side of the string to trim.
        side: smelt_hir::StringTrimSide,
        /// String operand to trim.
        operand: Operand,
    },
    /// Test whether a string has a prefix or suffix.
    StringAffix {
        /// Affix operation to apply.
        op: smelt_hir::StringAffixOp,
        /// String value to search in.
        haystack: Operand,
        /// Affix to search for.
        needle: Operand,
    },
    /// Find a substring index in a string, returning -1 when absent.
    StringSearch {
        /// Search operation to apply.
        op: smelt_hir::StringSearchOp,
        /// String value to search in.
        haystack: Operand,
        /// Substring to search for.
        needle: Operand,
        /// Optional JavaScript `fromIndex` position.
        from_index: Option<Operand>,
    },
    /// Replace literal string matches with a literal replacement string.
    StringReplace {
        /// Replacement operation to apply.
        op: smelt_hir::StringReplaceOp,
        /// String value to transform.
        haystack: Operand,
        /// Literal pattern to replace.
        pattern: Operand,
        /// Literal replacement value.
        replacement: Operand,
    },
    /// Remove a literal string prefix or suffix when it is present.
    StringRemoveAffix {
        /// Affix side to remove.
        op: smelt_hir::StringAffixOp,
        /// String value to transform.
        haystack: Operand,
        /// Affix to remove.
        affix: Operand,
    },
    /// Repeat a string a numeric number of times.
    StringRepeat {
        /// String value to repeat.
        operand: Operand,
        /// Number of repetitions.
        count: Operand,
    },
    /// Pad a string to a target length.
    StringPad {
        /// Padding side.
        op: smelt_hir::StringPadOp,
        /// String value to pad.
        operand: Operand,
        /// Target string length.
        target_len: Operand,
        /// Padding string.
        pad: Operand,
    },
    /// Test whether every character in a string satisfies a predicate.
    StringPredicate {
        /// Predicate to apply.
        op: smelt_hir::StringPredicateOp,
        /// String value to test.
        operand: Operand,
    },
    /// Test whether a regex pattern matches a string.
    RegexIsMatch {
        /// Regex match shape to apply.
        op: smelt_hir::RegexMatchOp,
        /// Regex pattern text.
        pattern: Operand,
        /// String value being matched.
        haystack: Operand,
    },
    /// Replace regex matches with a replacement string.
    RegexReplace {
        /// Replacement operation to apply.
        op: smelt_hir::StringReplaceOp,
        /// Regex pattern text.
        pattern: Operand,
        /// String value to transform.
        haystack: Operand,
        /// Replacement text.
        replacement: Operand,
    },
    /// Replace regex matches with the result of a callback.
    RegexReplaceCallback {
        /// Replacement operation to apply.
        op: smelt_hir::StringReplaceOp,
        /// Regex pattern text.
        pattern: Operand,
        /// String value to transform.
        haystack: Operand,
        /// Callback receiving the matched text and returning replacement text.
        callback: Operand,
    },
    /// Replace the first regex match with its uppercase text.
    RegexReplaceFirstMatchUppercase {
        /// Regex pattern text.
        pattern: Operand,
        /// String value to transform.
        haystack: Operand,
    },
    /// Split a string with a regex pattern.
    RegexSplit {
        /// Regex pattern text.
        pattern: Operand,
        /// String value to split.
        haystack: Operand,
    },
    /// Return the first regex match as a JavaScript-like match array.
    RegexFind {
        /// Regex pattern text.
        pattern: Operand,
        /// String value to search.
        haystack: Operand,
    },
    /// Execute a JavaScript-like `RegExp` value and return a match object.
    RegexExec {
        /// `RegExp` value.
        regex: Operand,
        /// String value to search.
        haystack: Operand,
    },
    /// Return every match index from JavaScript `String.prototype.matchAll`.
    RegexMatchAll {
        /// `RegExp` value.
        regex: Operand,
        /// String value to search.
        haystack: Operand,
    },
    /// Read a single character from a string as a string value.
    StringCharAt {
        /// String value to index.
        operand: Operand,
        /// Numeric character index.
        index: Operand,
    },
    /// Read a single character code from a string as a numeric value.
    StringCharCodeAt {
        /// String value to index.
        operand: Operand,
        /// Numeric character index.
        index: Operand,
    },
    /// Test whether one string contains another string.
    StringContains {
        /// String value to search in.
        haystack: Operand,
        /// Substring to search for.
        needle: Operand,
        /// Optional JavaScript `position` argument for `String.prototype.includes`.
        from_index: Option<Operand>,
    },
    /// Take a substring slice from a string.
    StringSlice {
        /// String value to slice.
        operand: Operand,
        /// Inclusive start index, or omitted for zero.
        start: Option<Operand>,
        /// Exclusive end index, or omitted for string length.
        end: Option<Operand>,
    },
    /// Test whether a list contains an item.
    ListContains {
        /// List value to search in.
        list: Operand,
        /// Item to search for.
        item: Operand,
    },
    /// Test whether a set contains an item.
    SetContains {
        /// Set value to search in.
        set: Operand,
        /// Item to search for.
        item: Operand,
    },
    /// Test whether two sets have no shared items.
    SetDisjoint {
        /// Left set operand.
        left: Operand,
        /// Right set operand.
        right: Operand,
    },
    /// Test a relation between two sets.
    SetRelation {
        /// Set relation to evaluate.
        op: smelt_hir::SetRelationOp,
        /// Left set operand.
        left: Operand,
        /// Right set operand.
        right: Operand,
    },
    /// Insert an item into a set.
    SetAdd {
        /// Set value to mutate.
        set: Operand,
        /// Item to insert.
        item: Operand,
    },
    /// Remove an item from a set.
    SetRemove {
        /// Missing-item behavior.
        op: smelt_hir::SetRemoveOp,
        /// Set value to mutate.
        set: Operand,
        /// Item to remove.
        item: Operand,
    },
    /// Clear all items from a set.
    SetClear {
        /// Set value to mutate.
        set: Operand,
    },
    /// Return a shallow copy of a set.
    SetCopy {
        /// Set value to copy.
        set: Operand,
    },
    /// Convert a list into a set by collecting unique items.
    ListToSet {
        /// List value to collect into a set.
        list: Operand,
    },
    /// Convert a list of key/value tuples into a dictionary.
    ListPairsToDict {
        /// List of pair tuples to collect into a dictionary.
        list: Operand,
    },
    /// Combine two sets into a new set.
    SetBinary {
        /// Set operation to apply.
        op: smelt_hir::SetBinaryOp,
        /// Left set operand.
        left: Operand,
        /// Right set operand.
        right: Operand,
    },
    /// Project values or entries from a set.
    SetProjection {
        /// Projection to apply.
        op: smelt_hir::SetProjectionOp,
        /// Set value to project.
        set: Operand,
    },
    /// Construct a WHATWG `URLSearchParams` value.
    UrlSearchParamsNew {
        /// Optional initializer value.
        init: Option<Operand>,
    },
    /// Apply a `URLSearchParams` operation to a concrete receiver.
    UrlSearchParamsOp {
        /// Operation to apply.
        op: smelt_hir::UrlSearchParamsOp,
        /// `URLSearchParams` receiver.
        params: Operand,
        /// Operation arguments (name, or name and value).
        args: Vec<Operand>,
    },
    /// Construct a WHATWG `Headers` value.
    ///
    /// The initializer's static type selects the conversion at emit time (a
    /// record, a list of name/value pairs, or another `Headers`), so no runtime
    /// tag test decides how the value is built.
    HeadersNew {
        /// Optional initializer value.
        init: Option<Operand>,
    },
    /// Apply a WHATWG `Headers` operation to a concrete `Headers` receiver.
    HeadersOp {
        /// Operation to apply.
        op: smelt_hir::HeadersOp,
        /// `Headers` receiver.
        headers: Operand,
        /// Operation arguments (name, or name and value).
        args: Vec<Operand>,
    },
    /// Concatenate two lists into a new list.
    ListConcat {
        /// Left list value.
        left: Operand,
        /// Right list value.
        right: Operand,
    },
    /// Normalize one erased `Array.prototype.concat` argument into the list of
    /// items it contributes, applying JavaScript's `IsConcatSpreadable` rule at
    /// runtime: an array splices its elements in, any other value appends as a
    /// single element.
    ConcatSpread {
        /// The erased argument value.
        value: Operand,
    },
    /// Find an item index in a list, returning -1 when absent.
    ListSearch {
        /// Search direction.
        op: smelt_hir::ListSearchOp,
        /// List value to search.
        list: Operand,
        /// Item to search for.
        item: Operand,
        /// Optional JavaScript `fromIndex` argument.
        from_index: Option<Operand>,
    },
    /// Apply a capture-free callback to a list operation.
    ListCallback {
        /// List callback operation to apply.
        op: smelt_hir::ListCallbackOp,
        /// List value to process.
        list: Operand,
        /// Closure or callable value.
        callback: Operand,
    },
    /// Build a sparse-like unknown list from a numeric length.
    ListFromLength {
        /// Numeric length source.
        length: Operand,
    },
    /// Build a list by cloning one value for a numeric count.
    ListRepeat {
        /// Value cloned into each list slot.
        value: Operand,
        /// Numeric count source.
        count: Operand,
    },
    /// Build a list by invoking a mapper for indexes from zero to length.
    ListFromLengthMap {
        /// Numeric length source.
        length: Operand,
        /// Closure or callable value.
        callback: Operand,
    },
    /// Reduce a list with a capture-free reducer callback and initial value.
    ListReduce {
        /// List value to reduce.
        list: Operand,
        /// Initial accumulator value, or omitted for JavaScript-style first-item seeding.
        initial: Option<Operand>,
        /// Closure or callable value.
        callback: Operand,
    },
    /// Take a shallow slice from a list.
    ListSlice {
        /// List value to slice.
        list: Operand,
        /// Inclusive start index, or omitted for zero.
        start: Option<Operand>,
        /// Exclusive end index, or omitted for collection length.
        end: Option<Operand>,
    },
    /// Remove and optionally insert list items.
    ListSplice {
        /// List value to mutate or copy.
        list: Operand,
        /// Start index.
        start: Operand,
        /// Number of items to delete, or omitted to delete through the end.
        delete_count: Option<Operand>,
        /// Replacement items.
        items: Vec<MirListSpliceItem>,
        /// Whether to mutate the source list and return removed items.
        mutate: bool,
    },
    /// Fill a list range with one value.
    ListFill {
        /// List value to mutate.
        list: Operand,
        /// Value to assign.
        value: Operand,
        /// Inclusive start index, or omitted for zero.
        start: Option<Operand>,
        /// Exclusive end index, or omitted for collection length.
        end: Option<Operand>,
    },
    /// Copy a range inside a list.
    ListCopyWithin {
        /// List value to mutate.
        list: Operand,
        /// Target index.
        target: Operand,
        /// Source start index.
        start: Operand,
        /// Source end index, or omitted for collection length.
        end: Option<Operand>,
    },
    /// Return a copied list with one replaced item.
    ListWith {
        /// List value to copy.
        list: Operand,
        /// Index to replace.
        index: Operand,
        /// Replacement value.
        value: Operand,
    },
    /// Flatten a list to an optional JavaScript depth.
    ListFlat {
        /// List value to flatten.
        list: Operand,
        /// Explicit flatten depth, or omitted for JavaScript's depth of one.
        depth: Option<Operand>,
    },
    /// Project array keys, values, or entries.
    ListProjection {
        /// Projection operation.
        op: smelt_hir::ListProjectionOp,
        /// List value to project.
        list: Operand,
    },
    /// Push one item into a list and return the new length.
    ListPush {
        /// List value to mutate.
        list: Operand,
        /// Item to append.
        item: Operand,
    },
    /// Extend a list with items from another list and return `None`.
    ListExtend {
        /// List value to mutate.
        list: Operand,
        /// List supplying items to copy.
        other: Operand,
    },
    /// Insert one item into a list at an integer index and return `None`.
    ListInsert {
        /// List value to mutate.
        list: Operand,
        /// Integer index where the item is inserted.
        index: Operand,
        /// Item to insert.
        item: Operand,
    },
    /// Insert zero or more items at the front of a list and return the new length.
    ListUnshift {
        /// List value to mutate.
        list: Operand,
        /// Items to insert at the front.
        items: Vec<Operand>,
    },
    /// Reverse a list in place and return the language-specific result.
    ListReverse {
        /// List value to mutate.
        list: Operand,
    },
    /// Clear all items from a list and return `None`.
    ListClear {
        /// List value to mutate.
        list: Operand,
    },
    /// Return a shallow copy of a list.
    ListCopy {
        /// List value to copy.
        list: Operand,
    },
    /// Convert a homogeneous tuple into a list.
    TupleToList {
        /// Tuple value to collect into a list.
        tuple: Operand,
    },
    /// Convert a homogeneous list into a statically-sized tuple.
    ListToTuple {
        /// List value to collect into a tuple.
        list: Operand,
    },
    /// Convert a homogeneous tuple into a set by collecting unique items.
    TupleToSet {
        /// Tuple value to collect into a set.
        tuple: Operand,
    },
    /// Count list items equal to a target item.
    ListCount {
        /// List value to scan.
        list: Operand,
        /// Item to count.
        item: Operand,
    },
    /// Sum numeric items in a list.
    ListSum {
        /// List value to reduce.
        list: Operand,
    },
    /// Fold boolean items in a list with `all` or `any` semantics.
    ListBoolFold {
        /// Fold operation to apply.
        op: smelt_hir::BoolFoldOp,
        /// Boolean list value to reduce.
        list: Operand,
    },
    /// Return a sorted copy of a list.
    ListSorted {
        /// List value to copy and sort.
        list: Operand,
        /// Optional key closure for Python `sorted(values, key=...)`.
        ///
        /// The key is a normal closure operand mapping one list item to a
        /// sortable value.
        key: Option<Operand>,
        /// Whether to sort in descending order, as in Python `reverse=True`.
        reverse: bool,
    },
    /// Return a reversed copy of a list.
    ListReversed {
        /// List value to copy in reverse order.
        list: Operand,
    },
    /// Pair list items with zero-based integer indexes.
    ListEnumerate {
        /// List value to enumerate.
        list: Operand,
    },
    /// Pair two lists elementwise, truncating to the shorter list.
    ListZip {
        /// Left list value to zip.
        left: Operand,
        /// Right list value to zip.
        right: Operand,
    },
    /// Materialize an integer range as a list.
    ListRange {
        /// Inclusive start value.
        start: Operand,
        /// Exclusive end value.
        end: Operand,
        /// Step value.
        step: Operand,
    },
    /// Pick one item from a list with a pseudo-random index.
    ListRandomChoice {
        /// List value to choose from.
        list: Operand,
    },
    /// Return the first index of an equal list item.
    ListIndex {
        /// List value to scan.
        list: Operand,
        /// Item to locate.
        item: Operand,
    },
    /// Remove the first list item equal to a target item and return `None`.
    ListRemove {
        /// List value to mutate.
        list: Operand,
        /// Item to remove.
        item: Operand,
    },
    /// Sort a list in place and return `None`.
    ListSort {
        /// List value to mutate.
        list: Operand,
        /// Optional comparator closure for JavaScript-style sort.
        ///
        /// The comparator is a normal closure operand, like other list
        /// callbacks; it takes two list items and returns a number.
        comparator: Option<Operand>,
        /// Optional key closure for Python `list.sort(key=...)`.
        ///
        /// The key is a normal closure operand mapping one list item to a
        /// sortable value. It is mutually exclusive with `comparator`.
        key: Option<Operand>,
        /// Whether to sort in descending order, as in Python `reverse=True`.
        reverse: bool,
    },
    /// Pop the last item from a list.
    ListPop {
        /// List value to mutate.
        list: Operand,
    },
    /// Remove and return the first item from a list.
    ListShift {
        /// List value to mutate.
        list: Operand,
    },
    /// Consume the first list item and return an erased iterator-result object.
    ListNext {
        /// List value to mutate.
        list: Operand,
    },
    /// Test whether a typed iterator result is exhausted.
    IteratorDone {
        /// Optional iterator item.
        result: Operand,
    },
    /// Read the optional item from a typed iterator result.
    IteratorValue {
        /// Optional iterator item.
        result: Operand,
    },
    /// Test whether a tuple contains an item.
    TupleContains {
        /// Tuple value to search in.
        tuple: Operand,
        /// Item to search for.
        item: Operand,
    },
    /// Read one statically-known tuple field by index.
    TupleIndex {
        /// Tuple value to read from.
        tuple: Operand,
        /// Zero-based tuple item index.
        index: usize,
    },
    /// Build a tuple from a statically-known slice of another tuple.
    TupleSlice {
        /// Tuple value to slice.
        tuple: Operand,
        /// Inclusive normalized start index.
        start: usize,
        /// Exclusive normalized end index.
        end: usize,
    },
    /// Test whether a dictionary contains a key.
    DictContainsKey {
        /// Dictionary value to search in.
        dict: Operand,
        /// Key to search for.
        key: Operand,
        /// How far the presence test may look: own properties only
        /// (`Object.hasOwn`, a typed collection probe) or the whole prototype
        /// chain (the `in` operator).
        lookup: PropertyLookup,
    },
    /// Insert or replace a dictionary key-value pair.
    DictSet {
        /// Dictionary value to mutate.
        dict: Operand,
        /// Key to write.
        key: Operand,
        /// Value to write.
        value: Operand,
    },
    /// Remove a dictionary key and return whether it existed.
    DictRemoveKey {
        /// Dictionary value to mutate.
        dict: Operand,
        /// Key to remove.
        key: Operand,
    },
    /// Look up a dictionary key and return an optional value or default.
    DictGet {
        /// Dictionary value to read.
        dict: Operand,
        /// Key to look up.
        key: Operand,
        /// Optional default value for missing keys.
        default: Option<Operand>,
    },
    /// Return an existing dictionary value or insert and return a default.
    DictSetDefault {
        /// Dictionary value to mutate.
        dict: Operand,
        /// Key to look up or insert.
        key: Operand,
        /// Default value to insert for a missing key.
        default: Operand,
    },
    /// Clear all entries from a dictionary and return `None`.
    DictClear {
        /// Dictionary value to mutate.
        dict: Operand,
    },
    /// Remove a dictionary key and return its value.
    DictPop {
        /// Dictionary value to mutate.
        dict: Operand,
        /// Key to remove.
        key: Operand,
        /// Optional default value for missing keys.
        default: Option<Operand>,
    },
    /// Extend a dictionary with entries from another dictionary and return `None`.
    DictUpdate {
        /// Dictionary value to mutate.
        dict: Operand,
        /// Dictionary supplying entries to copy.
        other: Operand,
    },
    /// Merge source dictionaries into a target dictionary and return the target.
    DictAssign {
        /// Dictionary receiving copied entries.
        target: Operand,
        /// Dictionaries supplying entries to copy.
        sources: Vec<Operand>,
    },
    /// Attach object-literal properties to a callable JavaScript value.
    CallableObjectAssign {
        /// Callable value receiving the properties.
        callable: Operand,
        /// Static property values to attach.
        props: Vec<(Symbol, Operand)>,
        /// Record values whose own enumerable entries are copied onto the
        /// callable object at runtime (dynamic `Object.assign` sources).
        spreads: Vec<Operand>,
    },
    /// Return a shallow copy of a dictionary.
    DictCopy {
        /// Dictionary value to copy.
        dict: Operand,
    },
    /// Project keys, values, or entries from a dictionary.
    DictProjection {
        /// Projection to apply.
        op: smelt_hir::DictProjectionOp,
        /// Dictionary value to project.
        dict: Operand,
    },
    /// Split a string into a list of strings.
    StringSplit {
        /// String value to split.
        haystack: Operand,
        /// Separator string.
        separator: Operand,
        /// Optional maximum number of pieces.
        limit: Option<Operand>,
    },
    /// Convert a string into one-character strings.
    StringChars {
        /// String value to expand.
        haystack: Operand,
    },
    /// Join a list of strings with a separator string.
    StringJoin {
        /// String items to join.
        items: Operand,
        /// Separator string.
        separator: Operand,
    },
    /// Serialize a JSON-compatible value to a JSON string.
    JsonStringify {
        /// Value to serialize.
        value: Operand,
    },
    /// Parse JSON text into a statically known JSON-compatible type.
    JsonParse {
        /// JSON text to parse.
        text: Operand,
    },
    /// Perform a blocking HTTP GET request and return response text.
    HttpGetText {
        /// URL to request.
        url: Operand,
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
    /// Read the current timestamp in milliseconds.
    DateNow,
    /// Configure the timestamp returned by JavaScript `Date.now()`.
    DateSetNow {
        /// Timestamp or Date-compatible value to use as the mocked clock.
        timestamp: Operand,
    },
    /// Restore the real JavaScript `Date.now()` clock.
    DateResetNow,
    /// Read the configured JavaScript `Date.prototype.getTimezoneOffset` value.
    DateTimezoneOffset,
    /// Configure the return value observed by `Date.prototype.getTimezoneOffset`.
    DateSetTimezoneOffset {
        /// Offset from UTC in minutes.
        offset: Operand,
    },
    /// Restore the default `Date.prototype.getTimezoneOffset` implementation.
    DateResetTimezoneOffset,
    /// Construct a stateful Vitest `vi.fn([impl])` mock object.
    VitestMockFn {
        /// Optional wrapped implementation used as the default outcome.
        implementation: Option<Operand>,
    },
    /// Whether a Vitest mock's recorded call count equals `count` (bool).
    VitestMockCalledTimes {
        /// The (possibly non-mock) actual value under assertion.
        mock: Operand,
        /// Expected call count.
        count: Operand,
    },
    /// Whether a Vitest mock recorded a call deep-equal to `args` (bool).
    /// When `last` is set, only the most recent recorded call is compared.
    VitestMockCalledWith {
        /// The (possibly non-mock) actual value under assertion.
        mock: Operand,
        /// Expected call arguments.
        args: Vec<Operand>,
        /// Compare only the most recent recorded call (`toHaveBeenLastCalledWith`).
        last: bool,
    },
    /// `vi.restoreAllMocks()`: undo every installed spy, newest first.
    VitestRestoreAllMocks,
    /// `vi.spyOn(target, name)`: install a recording mock over the member and
    /// evaluate to it.
    VitestSpyOn {
        /// The object whose member is replaced.
        target: Operand,
        /// The member name.
        name: Operand,
    },
    /// Whether two values are deep-equal under the vitest matcher rules,
    /// where either side may hold an asymmetric matcher (bool).
    VitestAsymmetricEqual {
        /// The actual value under assertion.
        actual: Operand,
        /// The expected value, possibly an asymmetric matcher or containing one.
        expected: Operand,
    },
    /// Whether a Vitest mock's most recent result deep-equals `expected`
    /// after flattening a resolved promise (bool).
    VitestMockLastResolvedWith {
        /// The (possibly non-mock) actual value under assertion.
        mock: Operand,
        /// Expected resolved value.
        expected: Operand,
    },
    /// Create a date-fns-compatible date context function for an IANA time zone.
    DateTimezoneContext {
        /// IANA time zone name.
        timezone: Operand,
    },
    /// Convert a timestamp in milliseconds to an ISO-8601 string.
    DateToIsoString {
        /// Timestamp in milliseconds.
        timestamp_ms: Operand,
    },
    /// Convert a timestamp in milliseconds to JavaScript Date string output.
    DateToString {
        /// Timestamp in milliseconds.
        timestamp_ms: Operand,
    },
    /// Construct a timestamp in milliseconds from local Date constructor parts.
    DateFromParts {
        /// Date constructor parts: year, month, date, hours, minutes, seconds, milliseconds.
        parts: Vec<Operand>,
    },
    /// Construct a timestamp in milliseconds from one JavaScript Date constructor value.
    DateFromValue {
        /// Date constructor argument.
        value: Operand,
    },
    /// Read a local-time date component from a timestamp.
    DateGetPart {
        /// Component to read.
        part: smelt_hir::DatePart,
        /// Timestamp in milliseconds.
        timestamp_ms: Operand,
    },
    /// Return a timestamp with a local-time date component replaced.
    DateSetPart {
        /// Component to replace.
        part: smelt_hir::DatePart,
        /// Timestamp in milliseconds.
        timestamp_ms: Operand,
        /// Replacement values accepted by the corresponding JS setter.
        values: Vec<Operand>,
    },
    /// Extract a parsed URL field.
    UrlField {
        /// URL field to extract.
        field: smelt_hir::UrlField,
        /// URL string to parse.
        url: Operand,
    },
    /// Read a UTF-8 text file.
    FileReadText {
        /// File path to read.
        path: Operand,
    },
    /// Write a UTF-8 text file.
    FileWriteText {
        /// File path to write.
        path: Operand,
        /// Text content to write.
        text: Operand,
    },
    /// Construct a modeled host `Blob`/`File` marker record from constructor parts.
    BlobFromParts {
        /// Erased `BlobPart` array (strings and other `Blob`/`File` records).
        parts: Operand,
        /// Resolved MIME `type` string.
        blob_type: Operand,
        /// `File` name; present only for `new File(...)`, which also stamps
        /// the `__smelt_file` marker on top of `__smelt_blob`.
        name: Option<Operand>,
        /// `File` options `lastModified` milliseconds, when spelled.
        last_modified: Option<Operand>,
    },
    /// Construct a modeled host object of a registry identity from its
    /// constructor arguments, through the *same* runtime constructor the
    /// reflected `Object.getPrototypeOf(x).constructor` path uses.
    ///
    /// See `ExprKind::HostConstruct`: clone idioms reach a host constructor both
    /// directly and reflectively, and the records must be indistinguishable.
    HostConstruct {
        /// Host constructor's registry class name (`"ArrayBuffer"`, `"DataView"`).
        class_name: String,
        /// Spelled constructor arguments, erased.
        args: Vec<Operand>,
    },
    /// The single interned value for a global builtin name used as a value.
    ///
    /// See `ExprKind::BuiltinNamespace`: one object per global name, so
    /// `Blob === Blob` and `blob.constructor === Blob` hold.
    BuiltinNamespace {
        /// The global builtin's source name (`"Blob"`, `"Math"`).
        name: String,
    },
    /// The enclosing non-arrow function's `arguments` object, rebuilt from that
    /// function's own parameters.
    ///
    /// See `ExprKind::ArgumentsObject`: the elements live under index keys and
    /// `length` is non-enumerable, which is what makes an `arguments` object
    /// compare equal to the plain object with the same indexed properties.
    ArgumentsObject {
        /// Positional parameter reads, in declaration order.
        fixed: Vec<Operand>,
        /// The rest parameter's list, flattened onto the end when present.
        rest: Option<Operand>,
    },
    /// Read a modeled host constructor's global override slot
    /// (`globalThis.<class>`). Yields a native-handle marker when the slot is
    /// `Native`, the stored constructor when overridden, or JS `undefined` when
    /// set absent. See `Rvalue::HostGlobalWrite`.
    HostGlobalRead {
        /// Modeled host constructor whose override slot is read.
        class: Symbol,
    },
    /// Write a modeled host constructor's global override slot
    /// (`globalThis.<class> = value`); evaluates to the stored value. The
    /// runtime write helper classifies the value into the slot state (`Absent`
    /// for `undefined`, `Native` for a native-handle marker, `Ctor` otherwise).
    HostGlobalWrite {
        /// Modeled host constructor whose override slot is written.
        class: Symbol,
        /// The value being stored.
        value: Operand,
    },
    /// Whether a modeled host constructor's global override slot is present
    /// (`false` only when overridden to JS `undefined`). Bool-typed.
    HostGlobalPresent {
        /// Modeled host constructor whose override slot presence is tested.
        class: Symbol,
    },
    /// Await a future and produce its output.
    Await(Operand),
    /// Run a runtime-backed async operation.
    AsyncOp {
        /// Operation to perform.
        op: smelt_hir::AsyncOp,
        /// Operation inputs.
        args: Vec<Operand>,
    },
}

/// Protocol operation used to resume or abruptly complete a generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeneratorResumeKind {
    /// Continue execution normally.
    Next,
    /// Complete with a caller-supplied return value.
    Return,
    /// Inject a caller-supplied exception.
    Throw,
}

/// A MIR statement.
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
    /// Mark a local as live.
    StorageLive(LocalId),
    /// Mark a local as dead.
    StorageDead(LocalId),
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
    /// Abort control flow.
    Unreachable,
}

impl Terminator {
    /// Return the control-flow successor blocks of this terminator.
    ///
    /// Exception handlers count as successors so dataflow analyses see the
    /// throwing edge of a `Call`/`Await`. `Return`, `Throw`, and `Unreachable`
    /// have none.
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
            Self::Return(_) | Self::Throw(_) | Self::Unreachable => Vec::new(),
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
}

/// Builtin functions recognized by MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltinFn {
    /// Print to the console.
    ConsoleLog,
    /// Write exact text to stdout.
    ConsoleWrite,
    /// Write exact text to stderr.
    ConsoleErrorWrite,
    /// Parse JSON text (`JSON.parse`).
    ///
    /// A builtin rather than an rvalue because it is *fallible*: malformed text
    /// throws a catchable `SyntaxError` in JavaScript. Only `Terminator::Call`
    /// and `Terminator::Await` carry an `unwind` edge, so a fallible operation
    /// has to be a call to reach an enclosing `try`.
    JsonParse,
}
