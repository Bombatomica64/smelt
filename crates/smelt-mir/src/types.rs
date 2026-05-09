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

/// The MIR representation of a crate, containing all functions, classes, and interfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mir {
    /// All functions in the crate.
    pub functions: Vec<MirFunction>,
    /// All classes in the crate.
    pub classes: Vec<MirClass>,
    /// All interfaces in the crate.
    pub interfaces: Vec<MirInterface>,
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
    /// Apply a direct unary numeric function.
    NumericUnaryFunc {
        /// Operation to apply.
        op: smelt_hir::NumericUnaryFuncOp,
        /// Numeric operand to transform.
        operand: Operand,
    },
    /// Raise a floating-point base to a floating-point exponent.
    NumericPow {
        /// Base operand.
        base: Operand,
        /// Exponent operand.
        exponent: Operand,
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
    /// Test whether every character in a string satisfies a predicate.
    StringPredicate {
        /// Predicate to apply.
        op: smelt_hir::StringPredicateOp,
        /// String value to test.
        operand: Operand,
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
    /// Test whether a list contains an item.
    ListContains {
        /// List value to search in.
        list: Operand,
        /// Item to search for.
        item: Operand,
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
    /// Test whether a tuple contains an item.
    TupleContains {
        /// Tuple value to search in.
        tuple: Operand,
        /// Item to search for.
        item: Operand,
    },
    /// Test whether a dictionary contains a key.
    DictContainsKey {
        /// Dictionary value to search in.
        dict: Operand,
        /// Key to search for.
        key: Operand,
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
    /// Perform a blocking HTTP GET request and return response text.
    HttpGetText {
        /// URL to request.
        url: Operand,
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
