//! The crate's name plan: every declared item's Rust name, decided once.
//!
//! Built at the start of an emission, before any text is written, and read by
//! every declaration and every reference. Names are keyed by stable identity
//! (`Symbol` for types and methods, `FuncId` for functions), never by their
//! rendering, so a lookup cannot confuse two items that render alike.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Symbol, TypeId};
use smelt_mir::{FuncId, HirOrigin, Mir, MirClass, MirFunction, MirInterface};

use super::{NameScope, NameSpace, RustIdent, RustName, Spelling, render};
use crate::EmitError;

/// One generated type declaration: a crate-root struct and its impls.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TypeDecl<'mir> {
    /// An interface (object shape) that no class of the same name declares.
    Interface(&'mir MirInterface),
    /// A class. An interface of the same name merges into it, as TypeScript
    /// declaration merging does (see `classes::effective_class_fields`).
    Class(&'mir MirClass),
}

impl TypeDecl<'_> {
    /// The declaration's identity.
    pub(crate) const fn name(self) -> Symbol {
        match self {
            Self::Interface(interface) => interface.name,
            Self::Class(class) => class.name,
        }
    }
}

/// The crate's type declarations in the one order their names are claimed and
/// their storage is emitted: interfaces, then classes.
///
/// Each identity appears once, and a class owns its symbol over an interface
/// of the same name (the class has the constructor and the methods; the
/// interface merges into it as TypeScript declaration merging does).
///
/// Two declarations of ONE symbol are not a rendering collision: every
/// reference names the type by that symbol, so codegen cannot tell which one
/// it means. That is the MIR's class-name uniqueness (audit item C12; two test
/// modules of one library each declaring `class Foo` reach MIR as one symbol),
/// and the first declaration keeps answering for it here, exactly as the MIR's
/// own name index does. Two symbols that RENDER alike are a different case,
/// and [`NameScope`] renames them apart.
pub(crate) fn type_claim_order(mir: &Mir) -> Vec<TypeDecl<'_>> {
    let class_names = mir.classes.iter().map(|class| class.name).collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut order = Vec::with_capacity(mir.interfaces.len().saturating_add(mir.classes.len()));
    for interface in &mir.interfaces {
        if !class_names.contains(&interface.name) && seen.insert(interface.name) {
            order.push(TypeDecl::Interface(interface));
        }
    }
    for class in &mir.classes {
        if seen.insert(class.name) {
            order.push(TypeDecl::Class(class));
        }
    }
    order
}

/// Every declared item's Rust name.
#[derive(Debug, Clone)]
pub(crate) struct RustNames {
    /// The crate's type namespace, kept open for generated traits.
    types_scope: NameScope,
    /// Declared types (classes, interfaces) by symbol.
    types: HashMap<Symbol, RustName>,
    /// Free functions (and the entry point) by id.
    functions: HashMap<FuncId, RustName>,
    /// Free functions by rendered name: the inverse of `functions`, unique
    /// because the value namespace is one scope.
    functions_by_name: HashMap<String, FuncId>,
    /// Instance methods by method symbol, unique crate-wide so an override and
    /// a bound trait's method render the same in every `impl`.
    methods: HashMap<Symbol, RustName>,
    /// Static methods by id, unique within their class's `impl` block.
    statics: HashMap<FuncId, RustName>,
    /// Every crate-root value and type name a function body could shadow.
    body_visible: Vec<String>,
    /// Struct fields by field symbol (see [`plan_fields`]).
    fields: HashMap<Symbol, RustIdent>,
}

impl RustNames {
    /// Plan the names of every declared item of `mir`.
    ///
    /// `none_ty` identifies the binary's entry point (a nullary `main` that
    /// returns nothing), whose name is `main` itself. `reference_classes` own a
    /// companion `<Name>Inner` struct, which is claimed with them.
    pub(crate) fn plan(
        mir: &Mir,
        none_ty: TypeId,
        reference_classes: &HashSet<Symbol>,
    ) -> Result<Self, EmitError> {
        let mut types_scope = NameScope::new(NameSpace::Types);
        let mut types = HashMap::new();
        for decl in type_claim_order(mir) {
            let symbol = decl.name();
            let spelling = symbol_text(mir, symbol)?;
            let name = if reference_classes.contains(&symbol) {
                claim_with_companion(&mut types_scope, spelling, "Inner")
            } else {
                types_scope.claim(spelling)
            };
            types.insert(symbol, name);
        }

        let functions = plan_free_functions(mir, none_ty)?;
        let functions_by_name = functions
            .iter()
            .map(|(id, name)| (name.as_str().to_owned(), *id))
            .collect();
        let methods = plan_instance_methods(mir)?;
        let statics = plan_static_methods(mir, &methods)?;
        let body_visible = functions
            .values()
            .chain(types.values())
            .map(|name| name.as_str().to_owned())
            .collect();
        let fields = plan_fields(mir)?;
        Ok(Self { types_scope, types, functions, functions_by_name, methods, statics, body_visible, fields })
    }

    /// The Rust name of the declared type `symbol`.
    pub(crate) fn type_name(&self, symbol: Symbol) -> Result<&RustName, EmitError> {
        self.types
            .get(&symbol)
            .ok_or_else(|| EmitError::new("type name requested for an undeclared type"))
    }

    /// Claim a generated trait's name (`<Bound>Like`) in the type namespace.
    pub(crate) fn claim_generated_type(&mut self, spelling: &str) -> RustName {
        // Composed by the emitter from an already-claimed type name.
        self.types_scope.claim_minted(spelling)
    }

    /// The Rust name of the free function `id`.
    pub(crate) fn function(&self, id: FuncId) -> Option<&RustName> {
        self.functions.get(&id)
    }

    /// The free function whose Rust name is exactly `text`, if any.
    ///
    /// Sound only because a body's locals never take a crate function's name
    /// (see [`Self::body_scope`]): text equal to a function name is that
    /// function, not a local shadowing it.
    pub(crate) fn function_named(&self, text: &str) -> Option<FuncId> {
        self.functions_by_name.get(text).copied()
    }

    /// The Rust name of the instance method `method`, when a class declares it.
    pub(crate) fn method(&self, method: Symbol) -> Option<&RustName> {
        self.methods.get(&method)
    }

    /// The Rust name of the static method `id`.
    pub(crate) fn static_method(&self, id: FuncId) -> Option<&RustName> {
        self.statics.get(&id)
    }

    /// The Rust field `symbol` names in every generated struct declaring it,
    /// or its lone escape when no generated struct declares it.
    pub(crate) fn field(&self, mir: &Mir, symbol: Symbol) -> RustIdent {
        if let Some(planned) = self.fields.get(&symbol) {
            return planned.clone();
        }
        Spelling::of(mir, symbol).map_or_else(
            || RustIdent("field".to_owned()),
            super::lone_field,
        )
    }

    /// [`Self::field`] as text, for the field spellings composed into code.
    pub(crate) fn field_text(&self, mir: &Mir, symbol: Symbol) -> String {
        self.field(mir, symbol).into_string()
    }

    /// A fresh scope for one function body's locals.
    ///
    /// It holds every crate-root function and type name: a parameter `fooBar`
    /// rendered `foo_bar` would otherwise shadow the function `foo_bar` the
    /// body calls, and a local `Item` would shadow the tuple-struct constructor
    /// of a reference class `Item`.
    pub(crate) fn body_scope(&self) -> NameScope {
        let mut scope = NameScope::new(NameSpace::Locals);
        for name in &self.body_visible {
            scope.hold(name);
        }
        scope
    }
}

/// The spelling of `symbol` with its origin, or an error naming the missing
/// symbol.
fn symbol_text(mir: &Mir, symbol: Symbol) -> Result<Spelling<'_>, EmitError> {
    Spelling::of(mir, symbol).ok_or_else(|| EmitError::new("declaration has an unknown symbol"))
}

/// Claim `spelling` such that `<name><companion>` is free too, and hold it.
///
/// A candidate whose companion is taken stays claimed (and unused), so the
/// next claim moves on to the next suffix.
fn claim_with_companion(scope: &mut NameScope, spelling: Spelling<'_>, companion: &str) -> RustName {
    loop {
        let name = scope.claim(spelling);
        let companion_name = format!("{name}{companion}");
        if !scope.is_taken(&companion_name) {
            scope.hold(&companion_name);
            return name;
        }
    }
}

/// Whether `function` is emitted as a free function of the crate.
const fn is_free_function(function: &MirFunction) -> bool {
    matches!(function.origin, HirOrigin::Body(_))
}

/// Plan every free function's name in the crate value namespace.
///
/// A function whose rendering another free function shares takes its id as a
/// suffix, as does every module body (`__smelt_module_*`), so a name stays
/// stable when an unrelated declaration is added. The binary's entry point is
/// `main` itself.
fn plan_free_functions(
    mir: &Mir,
    none_ty: TypeId,
) -> Result<HashMap<FuncId, RustName>, EmitError> {
    let mut scope = NameScope::new(NameSpace::Values);
    let mut renderings = HashMap::<String, usize>::new();
    for function in mir.functions.iter().filter(|function| is_free_function(function)) {
        let rendering = render(symbol_text(mir, function.name)?, NameSpace::Values);
        let count = renderings.entry(rendering).or_insert(0);
        *count = count.saturating_add(1);
    }
    let mut names = HashMap::new();
    // The entry point first: nothing may take `main` from it.
    for function in mir.functions.iter().filter(|function| is_entry_point(mir, function, none_ty)) {
        let name = scope
            .claim_emitter_name("main")
            .map_err(|clash| EmitError::new(format!("binary entry point: {clash}")))?;
        names.insert(function.id, name);
    }
    for function in mir.functions.iter().filter(|function| is_free_function(function)) {
        if names.contains_key(&function.id) {
            continue;
        }
        let source = symbol_text(mir, function.name)?;
        let rendering = render(source, NameSpace::Values);
        let name = if renderings.get(&rendering).copied().unwrap_or(0) > 1
            || source.text().starts_with("__smelt_module_")
        {
            scope.claim_suffixed(source, function.id.0)
        } else {
            scope.claim_salted(source, function.id.0)
        };
        names.insert(function.id, name);
    }
    Ok(names)
}

/// Whether `function` is the binary's entry point: a non-test nullary `main`
/// returning nothing, which keeps the name `main`.
fn is_entry_point(mir: &Mir, function: &MirFunction, none_ty: TypeId) -> bool {
    is_free_function(function)
        && !function.is_test
        && function.return_ty == none_ty
        && mir.symbols.get(function.name) == Some("main")
}

/// Plan every instance method's name, crate-wide by method symbol.
///
/// One scope for every class: an override must keep its base method's name and
/// a bound trait names the method once for all implementors, so a method
/// symbol renders the same everywhere. Two symbols that render alike are
/// renamed apart even when no class declares both, which costs a suffix and
/// never a collision.
fn plan_instance_methods(mir: &Mir) -> Result<HashMap<Symbol, RustName>, EmitError> {
    let mut scope = NameScope::new(NameSpace::Members);
    let mut names = HashMap::new();
    for function in &mir.functions {
        if let HirOrigin::ClassMethod { method, .. } = function.origin
            && !names.contains_key(&method)
        {
            names.insert(method, scope.claim(symbol_text(mir, method)?));
        }
    }
    Ok(names)
}

/// Plan every static method's name inside its class's `impl` block, which it
/// shares with the constructor `new` and every instance method of the class.
fn plan_static_methods(
    mir: &Mir,
    methods: &HashMap<Symbol, RustName>,
) -> Result<HashMap<FuncId, RustName>, EmitError> {
    let mut names = HashMap::new();
    for class in &mir.classes {
        let mut scope = NameScope::new(NameSpace::Members);
        scope.hold("new");
        for method in crate::classes::effective_class_methods(mir, class) {
            let Some(function) = usize::try_from(method.0).ok().and_then(|index| mir.functions.get(index)) else {
                continue;
            };
            if let HirOrigin::ClassMethod { method: inherited, .. } = function.origin
                && let Some(name) = methods.get(&inherited)
            {
                scope.hold(name.as_str());
            }
        }
        for static_method in &class.static_methods {
            let Some(function) = usize::try_from(static_method.0).ok().and_then(|index| mir.functions.get(index))
            else {
                continue;
            };
            if let HirOrigin::ClassStaticMethod { method, .. } = function.origin {
                names.insert(function.id, scope.claim_salted(symbol_text(mir, method)?, function.id.0));
            }
        }
    }
    Ok(names)
}

/// Plan every struct field's name: one [`NameScope`] per generated struct.
///
/// Two source fields that render alike in one struct (`fooBar` and `foo_bar`,
/// two symbols) are renamed apart in that struct's scope, and a source field
/// carrying an emitter prefix is moved out of it, so it cannot meet the
/// emitter's own `_smelt_phantom` or a lowering's `__smelt_call` slot.
///
/// A field is read and written at sites that know its symbol but not always
/// the struct it belongs to (a structural conversion, a record adapter, a
/// bound trait), so a symbol must name ONE field across every struct. A rename
/// one struct forces is therefore that symbol's name everywhere, and the
/// structs are re-checked until no scope needs another rename; each pass only
/// lengthens names, so it settles.
fn plan_fields(mir: &Mir) -> Result<HashMap<Symbol, RustIdent>, EmitError> {
    let structs = type_claim_order(mir)
        .into_iter()
        .map(|decl| {
            let fields = match decl {
                TypeDecl::Class(class) => crate::classes::effective_class_fields(mir, class),
                TypeDecl::Interface(interface) => crate::classes::effective_interface_fields(mir, interface),
            };
            fields.into_iter().map(|field| field.name).collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut names = HashMap::<Symbol, String>::new();
    for _ in 0..=structs.len().saturating_add(1) {
        let mut changed = false;
        for fields in &structs {
            let mut scope = NameScope::new(NameSpace::Fields);
            for &symbol in fields {
                let claimed = match names.get(&symbol) {
                    Some(name) if !scope.is_taken(name) => {
                        scope.hold(name);
                        continue;
                    }
                    // Taken here by another field: lengthen THIS symbol's
                    // name (`<name>_1`), so a rename never shortens a name
                    // another struct already settled on.
                    Some(name) => {
                        changed = true;
                        scope.claim_minted(name)
                    }
                    None => scope.claim(symbol_text(mir, symbol)?),
                };
                names.insert(symbol, claimed.as_str().to_owned());
            }
        }
        if !changed {
            return Ok(names.into_iter().map(|(symbol, name)| (symbol, RustIdent(name))).collect());
        }
    }
    Err(EmitError::new("struct field names did not settle"))
}
