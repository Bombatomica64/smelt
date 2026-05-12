//! Core MIR type definitions and structures.
//!
//! This module defines the fundamental types and structures used in the MIR representation,
//! including functions, basic blocks, locals, and various statements and expressions.

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
    /// Type interner for interned types.
    pub types: smelt_hir::TypeInterner,
    /// Symbol interner for interned identifiers.
    pub symbols: smelt_hir::SymbolInterner,
}

impl Mir {
    /// Creates a new empty MIR crate with the given type and symbol interners.
    #[must_use]
    pub fn new(types: smelt_hir::TypeInterner, symbols: smelt_hir::SymbolInterner) -> Self {
        Self {
            functions: Vec::new(),
            classes: Vec::new(),
            interfaces: Vec::new(),
            closures: Vec::new(),
            types,
            symbols,
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
    /// Closure parameter locals.
    pub params: Vec<LocalDecl>,
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
    /// Callback-expression body used while legacy callback lowering is migrated.
    pub callback_body: Option<smelt_hir::CallbackExpr>,
}

/// One explicit MIR closure capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirClosureCapture {
    /// Source local captured from the enclosing function.
    pub source_local: LocalId,
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
    /// Single base class, if any (multiple inheritance is rejected upstream).
    pub base: Option<Symbol>,
    /// Fields defined in the class.
    pub fields: Vec<MirField>,
    /// Constructor function ID, if any.
    pub constructor: Option<FuncId>,
    /// Method function IDs.
    pub methods: Vec<FuncId>,
    /// Interfaces this class implements.
    pub implements: Vec<Symbol>,
}

/// MIR representation of an interface definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirInterface {
    /// Name of the interface.
    pub name: Symbol,
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

/// MIR representation of a function with basic blocks and locals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirFunction {
    /// Unique identifier of this function.
    pub id: FuncId,
    /// Name of the function.
    pub name: Symbol,
    /// Origin information (from HIR).
    pub origin: HirOrigin,
    /// Whether this is an async function.
    pub is_async: bool,
    /// Whether this function should be emitted as a native Rust test.
    pub is_test: bool,
    /// Whether this function can return through an uncaught throw path.
    pub can_throw: bool,
    /// Parameter local IDs.
    pub params: Vec<LocalId>,
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
            origin,
            is_async: false,
            is_test: false,
            can_throw: false,
            params: Vec::new(),
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
///
/// # Panics
///
/// Panics if the value does not fit in `u32`.
fn len_to_u32(len: usize, label: &str) -> u32 {
    match u32::try_from(len) {
        Ok(value) => value,
        Err(error) => panic!("{label} does not fit in u32: {error}"),
    }
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
    Param,
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
    },
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
    /// `None`.
    None,
}

/// An MIR rvalue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rvalue {
    /// Use an existing operand directly.
    Use(Operand),
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
    /// Test whether a value is an instance of a class.
    InstanceOf {
        /// Value being tested. Kept as an operand so side effects are evaluated before the check.
        value: Operand,
        /// Target class symbol.
        class: Symbol,
    },
    /// Test the runtime tag of a TypeScript `unknown` value.
    UnknownIs {
        /// Value being tested.
        value: Operand,
        /// Runtime tag to check.
        kind: smelt_hir::UnknownKind,
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
    /// Call a closure value.
    ClosureCall {
        /// Closure value to call.
        callee: Operand,
        /// Call arguments.
        args: Vec<Operand>,
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
    /// Split a string with a regex pattern.
    RegexSplit {
        /// Regex pattern text.
        pattern: Operand,
        /// String value to split.
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
    /// Concatenate two lists into a new list.
    ListConcat {
        /// Left list value.
        left: Operand,
        /// Right list value.
        right: Operand,
    },
    /// Find an item index in a list, returning -1 when absent.
    ListSearch {
        /// Search direction.
        op: smelt_hir::ListSearchOp,
        /// List value to search.
        list: Operand,
        /// Item to search for.
        item: Operand,
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
        items: Vec<Operand>,
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
    /// Flatten one list nesting level.
    ListFlat {
        /// List value to flatten.
        list: Operand,
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
        /// Optional comparator callback for JavaScript-style sort.
        comparator: Option<smelt_hir::CallbackExpr>,
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
    /// Read the current timestamp in milliseconds.
    DateNow,
    /// Convert a timestamp in milliseconds to an ISO-8601 string.
    DateToIsoString {
        /// Timestamp in milliseconds.
        timestamp_ms: Operand,
    },
    /// Construct a timestamp in milliseconds from local Date constructor parts.
    DateFromParts {
        /// Date constructor parts: year, month, date, hours, minutes, seconds, milliseconds.
        parts: Vec<Operand>,
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
}
