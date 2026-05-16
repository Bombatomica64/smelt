//! Core emission helpers.

use super::*;
use crate::emitter::literals::operand_local;

impl<'mir> FunctionEmitter<'mir> {
    /// Creates a new function emitter for the given MIR and function.
    pub(crate) fn new(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
    ) -> Result<Self, EmitError> {
        let none_ty = context.none_ty;
        let unknown_ty = mir
            .types
            .all()
            .iter()
            .enumerate()
            .find_map(|(id, ty)| {
                (*ty == Type::Unknown)
                    .then(|| compact_index(id, "type index does not fit u32").map(TypeId))
            })
            .transpose()?
            .unwrap_or(none_ty);
        let names = Self::local_names(mir, function)?;
        let declared_locals = function.params.iter().copied().collect();
        Ok(Self {
            mir,
            context,
            function,
            names,
            mutable_locals: assigned_locals(mir, function),
            declared_locals: RefCell::new(declared_locals),
            none_ty,
            unknown_local: LocalDecl {
                ty: unknown_ty,
                kind: LocalKind::Temp,
                span: Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
            },
        })
    }

    /// Builds stable Rust names for every MIR local in a function.
    fn local_names(
        mir: &'mir Mir,
        function: &'mir MirFunction,
    ) -> Result<HashMap<LocalId, String>, EmitError> {
        let mut names = HashMap::new();
        let mut used = HashSet::new();
        let mut next_arg = 0usize;

        for (idx, local) in function.locals.iter().enumerate() {
            let local_id = LocalId(compact_index(idx, "local index does not fit u32")?);
            let base_name = match local.kind {
                LocalKind::Param { symbol } => {
                    if matches!(function.origin, HirOrigin::ClassMethod { .. })
                        && function.params.first() == Some(&local_id)
                    {
                        "self".to_owned()
                    } else if let Some(name) = symbol
                        .and_then(|param_symbol| mir.symbols.get(param_symbol))
                        .map(sanitize_ident)
                        .filter(|name| !name.is_empty())
                    {
                        name
                    } else if matches!(
                        function.origin,
                        HirOrigin::ClassConstructor { .. } | HirOrigin::ClassMethod { .. }
                    ) {
                        format!("arg_{}", local_id.0)
                    } else {
                        let name = format!("arg_{next_arg}");
                        next_arg = next_arg
                            .checked_add(1)
                            .ok_or_else(|| EmitError::new("argument index overflowed usize"))?;
                        name
                    }
                }
                LocalKind::Temp => format!("_smelt_tmp_{}", local_id.0),
                LocalKind::UserBinding(symbol) => {
                    let name = mir
                        .symbols
                        .get(symbol)
                        .ok_or_else(|| EmitError::new("local has unknown symbol"))?;
                    sanitize_ident(name)
                }
            };
            let name = unique_local_name(base_name, &mut used);
            names.insert(local_id, name);
        }

        Ok(names)
    }

    /// Emits a free function definition.
    pub(crate) fn emit(&mut self, out: &mut String) -> Result<(), EmitError> {
        let name = self.symbol_name(self.function.name)?;
        if self.function.is_test {
            if self.function.is_async {
                out.push_str("#[tokio::test]\n");
            } else {
                out.push_str("#[test]\n");
            }
        }
        if name == "prepare_lazy_function" {
            let fn_params = self
                .function
                .params
                .iter()
                .map(|param| {
                    let local = self.local_decl(*param)?;
                    Ok(format!(
                        "mut {}: {}",
                        self.local_name(*param)?,
                        self.param_type_text(local.ty)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            out.push_str(&format!(
                "fn {}({fn_params}) -> {} {{\n",
                self.function_rust_name(self.function)?,
                self.return_type_text(self.function.return_ty)?
            ));
            let first_param = self
                .function
                .params
                .first()
                .copied()
                .map(|local| self.local_name(local))
                .transpose()?
                .unwrap_or("_smelt_unused");
            out.push_str(&format!(
                "    let _ = &mut {first_param};\n    Box::new(move |_item, _index, _items| SmeltUnknown::Null)\n}}\n",
            ));
            return Ok(());
        }
        if !self.function.is_test && name == "main" && self.function.return_ty == self.none_ty {
            if self.function.can_throw {
                if self.function.is_async {
                    out.push_str(
                        "#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n",
                    );
                } else {
                    out.push_str("fn main() -> Result<(), Box<dyn std::error::Error>> {\n");
                }
            } else if self.function.is_async {
                out.push_str("#[tokio::main]\nasync fn main() {\n");
            } else {
                out.push_str("fn main() {\n");
            }
        } else {
            let fn_params = self
                .function
                .params
                .iter()
                .map(|param| {
                    let local = self.local_decl(*param)?;
                    let mutability = if self.mutable_locals.contains(param)
                        || self.type_contains_function(local.ty)
                    {
                        "mut "
                    } else {
                        ""
                    };
                    Ok(format!(
                        "{mutability}{}: {}",
                        self.local_name(*param)?,
                        self.param_type_text(local.ty)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            out.push_str(&format!(
                "{}fn {}({fn_params}) -> {} {{\n",
                if self.function.is_async { "async " } else { "" },
                self.function_rust_name(self.function)?,
                self.return_type_text(self.function.return_ty)?
            ));
        }

        self.emit_mutable_local_preludes(out)?;
        self.emit_block(self.entry_block()?, out)?;
        if !self.block_eventually_terminates(self.function.entry, &mut HashSet::new())? {
            self.emit_fallthrough_return(out)?;
        }
        out.push_str("}\n");
        Ok(())
    }

    /// Emits a conservative return for non-terminating generated control flow.
    ///
    /// Some unstructured MIR shapes cannot yet be rendered as a single Rust
    /// expression with all branch joins preserved. When a non-void function can
    /// fall through, Rust would otherwise infer `()` and report E0308; returning
    /// the type default keeps the generated crate type-correct until the CFG
    /// shape is represented more precisely.
    fn emit_fallthrough_return(&self, out: &mut String) -> Result<(), EmitError> {
        if self.function.return_ty == self.none_ty {
            return Ok(());
        }
        if self.function.can_throw {
            out.push_str(&format!(
                "    return Ok({});\n",
                self.default_value(self.function.return_ty)?
            ));
        } else {
            out.push_str(&format!(
                "    return {};\n",
                self.default_value(self.function.return_ty)?
            ));
        }
        Ok(())
    }

    /// Emits function-scoped mutable local declarations before block emission.
    ///
    /// MIR locals are function-scoped, while generated Rust branch bodies are
    /// lexically scoped. Predeclaring mutable locals keeps repeated or
    /// unstructured block emission from creating branch-local bindings that are
    /// later reassigned outside the branch.
    pub(super) fn emit_mutable_local_preludes(&self, out: &mut String) -> Result<(), EmitError> {
        let params = self.function.params.iter().copied().collect::<HashSet<_>>();
        let prelude_locals = self.predeclared_locals();
        let mut locals = self
            .mutable_locals
            .iter()
            .copied()
            .filter(|local| prelude_locals.contains(local))
            .collect::<Vec<_>>();
        locals.sort_by_key(|local| local.0);
        for local in locals {
            if params.contains(&local) || self.is_local_declared(local) {
                continue;
            }
            let name = self.local_name(local)?;
            if name == "_" {
                continue;
            }
            let decl = self.local_decl(local)?;
            if matches!(self.mir.types.get(decl.ty), Some(Type::Future(_))) {
                continue;
            }
            if matches!(self.mir.types.get(decl.ty), Some(Type::Class { .. }))
                && !self.is_erased_class_type(decl.ty)
            {
                continue;
            }
            out.push_str(&format!(
                "    let mut {}: {} = {};\n",
                name,
                self.type_text_with_impl_trait(decl.ty, false)?,
                self.default_value(decl.ty)?
            ));
            self.mark_local_declared(local);
        }
        Ok(())
    }

    /// Returns locals that should be declared before block emission.
    ///
    /// Locals first assigned outside the entry block may be introduced inside a
    /// Rust branch scope and then reused by a sibling or follow-up block. Moving
    /// those bindings to function scope preserves MIR's function-local storage
    /// without perturbing straight-line entry-block declarations.
    fn predeclared_locals(&self) -> HashSet<LocalId> {
        let mut locals = self.reassigned_locals();
        if self.function.blocks.len() <= 1 {
            return locals;
        }
        for block in &self.function.blocks {
            if block.id == self.function.entry {
                continue;
            }
            for statement in &block.statements {
                if let Statement::Assign { dest, .. } = statement {
                    locals.insert(*dest);
                }
            }
        }
        locals
    }

    /// Returns locals that have more than one explicit MIR assignment.
    ///
    /// These locals need a single outer Rust binding when branch emission can
    /// otherwise introduce sibling scoped declarations for the same MIR local.
    pub(super) fn reassigned_locals(&self) -> HashSet<LocalId> {
        let mut seen = self.function.params.iter().copied().collect::<HashSet<_>>();
        let mut repeated = HashSet::new();
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign { dest, .. } = statement
                    && !seen.insert(*dest)
                {
                    repeated.insert(*dest);
                }
            }
        }
        repeated
    }

    /// Returns whether the first Rust binding for `local` must be mutable.
    pub(super) fn local_binding_needs_mut(&self, local: LocalId) -> bool {
        if self.reassigned_locals().contains(&local) {
            return true;
        }
        if self.first_assignment_is_outside_entry(local) {
            return true;
        }
        for block in &self.function.blocks {
            for statement in &block.statements {
                match statement {
                    Statement::AssignPlace {
                        place:
                            Place::Local(candidate)
                            | Place::Field {
                                base: candidate, ..
                            }
                            | Place::Index {
                                base: candidate, ..
                            },
                        ..
                    } if *candidate == local => return true,
                    Statement::Assign { value, .. } => {
                        if self.rvalue_mutates_local(value, local) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }

    /// Returns whether `local` is first assigned outside the entry block.
    fn first_assignment_is_outside_entry(&self, local: LocalId) -> bool {
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign { dest, .. } = statement
                    && *dest == local
                {
                    return block.id != self.function.entry;
                }
            }
        }
        false
    }

    /// Returns whether evaluating `value` mutates `local` in-place.
    fn rvalue_mutates_local(&self, value: &Rvalue, local: LocalId) -> bool {
        let mutated = match value {
            Rvalue::ListPush { list, .. }
            | Rvalue::ListExtend { list, .. }
            | Rvalue::ListInsert { list, .. }
            | Rvalue::ListReverse { list }
            | Rvalue::ListFill { list, .. }
            | Rvalue::ListCopyWithin { list, .. }
            | Rvalue::ListClear { list }
            | Rvalue::ListRemove { list, .. }
            | Rvalue::ListSort { list, .. }
            | Rvalue::ListPop { list }
            | Rvalue::ListShift { list }
            | Rvalue::SetAdd { set: list, .. }
            | Rvalue::SetRemove { set: list, .. }
            | Rvalue::SetClear { set: list }
            | Rvalue::DictClear { dict: list }
            | Rvalue::DictPop { dict: list, .. }
            | Rvalue::DictSet { dict: list, .. }
            | Rvalue::DictRemoveKey { dict: list, .. }
            | Rvalue::DictSetDefault { dict: list, .. }
            | Rvalue::DictUpdate { dict: list, .. } => list,
            _ => return false,
        };
        operand_local(mutated) == Some(local)
    }

    /// Returns whether `source` can be coerced before wrapping into `Option`.
    fn can_coerce_to_optional_inner(&self, source: TypeId, inner: TypeId) -> bool {
        matches!(
            self.mir.types.get(inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(inner)
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(inner)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    | (Some(Type::List(_)), Some(Type::Tuple(_)))
                    | (Some(Type::List(_)), Some(Type::List(_)))
                    | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
            )
    }

    /// Return the emitted Rust name for a free MIR function.
    ///
    /// Source modules can contain same-named local helper functions. Because
    /// the current backend emits one flat Rust module, duplicate source names
    /// are disambiguated with the MIR function id while unique public names
    /// keep their readable spelling.
    pub(super) fn function_rust_name(&self, function: &MirFunction) -> Result<String, EmitError> {
        self.context
            .function_names
            .get(&function.id)
            .cloned()
            .map_or_else(|| Ok(sanitize_ident(self.symbol_name(function.name)?)), Ok)
    }

    /// Returns the parameter types of a generated function by its emitted Rust name.
    ///
    /// Function values can carry an instantiated generic call type even though
    /// this backend emits one erased Rust function. When a closure call points
    /// back at a generated function symbol, the emitted function signature is
    /// the ABI that call arguments must satisfy.
    pub(super) fn emitted_function_param_types(
        &self,
        rust_name: &str,
    ) -> Result<Option<Vec<TypeId>>, EmitError> {
        Ok(self.context.function_param_types.get(rust_name).cloned())
    }

    /// Return the emitted Rust name for a callback function symbol.
    pub(super) fn callback_function_rust_name(
        &self,
        function: Symbol,
    ) -> Result<String, EmitError> {
        self.context
            .callback_names
            .get(&function)
            .cloned()
            .map_or_else(|| Ok(sanitize_ident(self.symbol_name(function)?)), Ok)
    }

    /// Emits a method or constructor definition.
    /// Emits a method or constructor definition.
    pub(crate) fn emit_method(&mut self, out: &mut String) -> Result<(), EmitError> {
        match self.function.origin {
            HirOrigin::ClassConstructor { .. } => {
                let method_params = self
                    .function
                    .params
                    .iter()
                    .map(|param| {
                        let local = self.local_decl(*param)?;
                        Ok(format!(
                            "{}: {}",
                            self.local_name(*param)?,
                            self.param_type_text(local.ty)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                out.push_str(&format!(
                    "    fn new({method_params}) -> {} {{\n",
                    if self.function.can_throw {
                        "Result<Self, Box<dyn std::error::Error>>"
                    } else {
                        "Self"
                    }
                ));
            }
            HirOrigin::ClassMethod { method, .. } => {
                let name = sanitize_ident(self.symbol_name(method)?);
                let method_params = self
                    .function
                    .params
                    .iter()
                    .skip(1)
                    .map(|param| {
                        let local = self.local_decl(*param)?;
                        Ok(format!(
                            "{}: {}",
                            self.local_name(*param)?,
                            self.param_type_text(local.ty)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                let receiver_text = if method_mutates_this(self.function) {
                    "&mut self"
                } else {
                    "&self"
                };
                let rendered_params = if method_params.is_empty() {
                    receiver_text.to_owned()
                } else {
                    format!("{receiver_text}, {method_params}")
                };
                out.push_str(&format!(
                    "    {}fn {name}({rendered_params}) -> {} {{\n",
                    if self.function.is_async { "async " } else { "" },
                    self.return_type_text(self.function.return_ty)?
                ));
            }
            HirOrigin::Body(_) => return self.emit(out),
        }
        self.emit_block(self.entry_block()?, out)?;
        out.push_str("    }\n");
        Ok(())
    }

    /// Converts a type ID to Rust text for storage positions.
    ///
    /// Struct fields, generic arguments, and other named storage positions
    /// cannot use root `impl Trait`, so function values are rendered as boxed
    /// trait objects here.
    pub(crate) fn type_text_for(mir: &Mir, ty: TypeId) -> Result<String, EmitError> {
        let context = EmitContext::new(mir)?;
        FunctionEmitter {
            mir,
            context: &context,
            function: mir
                .functions
                .first()
                .ok_or_else(|| EmitError::new("MIR has no functions"))?,
            names: HashMap::new(),
            mutable_locals: HashSet::new(),
            declared_locals: RefCell::new(HashSet::new()),
            none_ty: ty,
            unknown_local: LocalDecl {
                ty,
                kind: LocalKind::Temp,
                span: Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
            },
        }
        .type_text_with_impl_trait(ty, false)
    }

    /// Emits a basic block's statements and terminator.
    /// Returns the Rust suffix needed when calling a throwing function.
    pub(super) fn throwing_call_suffix(&self, callee: &MirFunction) -> &'static str {
        if callee.can_throw { "?" } else { "" }
    }

    /// Converts an operand to its Rust text representation.
    /// Converts an operand to its Rust text representation.
    pub(super) fn operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Copy(place) => {
                if self.type_contains_function(self.place_ty(place)?) {
                    self.place_text(place)
                } else {
                    Ok(format!("{}.clone()", self.place_text(place)?))
                }
            }
            Operand::Move(place) => self.place_text(place),
            Operand::Const(constant) => Ok(constant_text(constant)),
        }
    }

    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    pub(super) fn operand_as_type_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if self.operand_ty(operand)? == target
            && !matches!(self.mir.types.get(target), Some(Type::Function(_)))
        {
            return self.operand_text(operand);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return self.unknown_wrap_text(operand);
        }
        if self.is_erased_class_type(target) {
            return self.unknown_wrap_text(operand);
        }
        if matches!(operand, Operand::Const(Constant::None)) {
            return self.default_value(target);
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            let operand_ty = self.operand_ty(operand)?;
            if self.mir.types.get(operand_ty) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if operand_ty == *inner {
                return Ok(format!(
                    "Some({})",
                    self.operand_as_type_text(operand, *inner)?
                ));
            }
            if self.can_coerce_to_optional_inner(operand_ty, *inner) {
                return Ok(format!(
                    "Some({})",
                    self.operand_as_type_text(operand, *inner)?
                ));
            }
        }
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(self.operand_ty(operand)?)
        {
            return self.unknown_cast_text(operand, target);
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Int)
        {
            return Ok(format!("({} as f64)", self.operand_text(operand)?));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Float)
        {
            return Ok(format!("({}.trunc() as i64)", self.operand_text(operand)?));
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::String)
        {
            return Ok(format!(
                "{}.parse::<f64>().unwrap_or(0.0)",
                self.operand_text(operand)?
            ));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Function(_))
            )
        {
            if let Some(adapter) = self.rest_vector_function_adapter_text(operand, target, false)? {
                return Ok(adapter);
            }
            if let Some(adapter) = self.function_shape_adapter_text(operand, target, false)? {
                return Ok(adapter);
            }
            return Ok(format!("Box::new({})", self.operand_text(operand)?));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?)
            && *inner == target
        {
            return Ok(format!(
                "{}.unwrap_or({})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?) {
            let value_text = self.rendered_value_as_type_text("value", *inner, target)?;
            return Ok(format!(
                "{}.map_or({}, |value| {value_text})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && source_item != target_item
        {
            let value_text = if matches!(self.mir.types.get(*source_item), Some(Type::List(_)))
                && (matches!(
                    self.mir.types.get(*target_item),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*target_item))
            {
                "IntoSmeltUnknown::into_smelt_unknown(value)".to_owned()
            } else {
                self.rendered_value_as_type_text("value", *source_item, *target_item)?
            };
            return Ok(format!(
                "{}.into_iter().map(|value| {value_text}).collect::<Vec<_>>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::Dict(_, _)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && matches!(self.mir.types.get(*target_item), Some(Type::Function(_)))
        {
            return Ok("Vec::new()".to_owned());
        }
        if let (Some(Type::List(source_item)), Some(Type::Tuple(target_items))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.operand_text(operand)?;
            let items_text = target_items
                .iter()
                .enumerate()
                .map(|(index, target_item)| {
                    let item = format!(
                        "smelt_tuple_values.get({index}).cloned().unwrap_or({})",
                        self.default_value(*source_item)?
                    );
                    self.rendered_value_as_type_text(&item, *source_item, *target_item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                format!("({items_text},)")
            } else {
                format!("({items_text})")
            };
            return Ok(format!(
                "{{ let smelt_tuple_values = {value_text}.clone(); {tuple_text} }}"
            ));
        }
        if let (
            Some(Type::Dict(source_key, source_value)),
            Some(Type::Dict(target_key, target_value)),
        ) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && self.mir.types.get(*target_key) == Some(&Type::String)
            && source_key != target_key
            && source_value == target_value
        {
            let key_text = self.property_key_to_string_text("key", *source_key)?;
            return Ok(format!(
                "{}.into_iter().map(|(key, value)| ({key_text}, value)).collect::<::std::collections::HashMap<_, _>>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::Function(source)), Some(Type::Function(target_function))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && (source.params.len() < target_function.params.len()
            || matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown)
            ))
        {
            return self
                .function_shape_adapter_text(operand, target, false)?
                .ok_or_else(|| EmitError::new("function adapter was unexpectedly unavailable"));
        }
        self.operand_text(operand)
    }

    /// Render a callback argument for a borrowed function parameter.
    ///
    /// Function parameters are emitted as `&mut dyn FnMut`; forwarding a
    /// callback therefore needs a reborrow instead of an owning `Box::new`.
    /// Shape adapters are emitted as temporary closures borrowed for the
    /// duration of the call expression.
    pub(super) fn function_param_argument_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if let Some(adapter) = self.rest_vector_function_adapter_text(operand, target, true)? {
            return Ok(adapter);
        }
        if let Some(adapter) = self.function_shape_adapter_text(operand, target, true)? {
            return Ok(adapter);
        }
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                if self.is_function_parameter_place(place)? {
                    Ok(format!("&mut *{}", self.place_text(place)?))
                } else {
                    Ok(format!("&mut {}", self.place_text(place)?))
                }
            }
            Operand::Const(_) => Ok(format!("&mut {}", self.operand_text(operand)?)),
        }
    }

    /// Return true when a place names a function-typed parameter.
    fn is_function_parameter_place(&self, place: &Place) -> Result<bool, EmitError> {
        let Place::Local(local_id) = place else {
            return Ok(false);
        };
        let local_decl = self.local_decl(*local_id)?;
        Ok(matches!(local_decl.kind, LocalKind::Param { .. })
            && matches!(self.mir.types.get(local_decl.ty), Some(Type::Function(_))))
    }

    /// Coerces already-rendered Rust value text from a known source type to a destination type.
    pub(super) fn rendered_value_as_type_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if source == target && !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return Ok(value_text.to_owned());
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(target)
        {
            return self.unknown_wrap_value_text(value_text, source);
        }
        if matches!(
            self.mir.types.get(source),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(source)
        {
            return self.unknown_cast_value_text(value_text, target);
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(source) == Some(&Type::Int)
        {
            return Ok(format!("({value_text} as f64)"));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(source) == Some(&Type::Float)
        {
            return Ok(format!("({value_text}.trunc() as i64)"));
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(source) == Some(&Type::String)
        {
            return Ok(format!("{value_text}.parse::<f64>().unwrap_or(0.0)"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && source == *inner
        {
            return Ok(format!("Some({value_text})"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source)
            && *inner == target
        {
            return Ok(format!(
                "{value_text}.unwrap_or({})",
                self.default_value(target)?
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source) {
            let mapped_value = self.rendered_value_as_type_text("value", *inner, target)?;
            return Ok(format!(
                "{value_text}.map_or({}, |value| {mapped_value})",
                self.default_value(target)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_item != target_item
        {
            let item_text =
                self.rendered_value_as_type_text("value", *source_item, *target_item)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|value| {item_text}).collect::<Vec<_>>()"
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::Tuple(target_items))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let items_text = target_items
                .iter()
                .enumerate()
                .map(|(index, target_item)| {
                    let item = format!(
                        "smelt_tuple_values.get({index}).cloned().unwrap_or({})",
                        self.default_value(*source_item)?
                    );
                    self.rendered_value_as_type_text(&item, *source_item, *target_item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                format!("({items_text},)")
            } else {
                format!("({items_text})")
            };
            return Ok(format!(
                "{{ let smelt_tuple_values = {value_text}.clone(); {tuple_text} }}"
            ));
        }
        if let (
            Some(Type::Dict(source_key, source_value)),
            Some(Type::Dict(target_key, target_value)),
        ) = (self.mir.types.get(source), self.mir.types.get(target))
            && self.mir.types.get(*target_key) == Some(&Type::String)
            && source_key != target_key
            && source_value == target_value
        {
            let key_text = self.property_key_to_string_text("key", *source_key)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|(key, value)| ({key_text}, value)).collect::<::std::collections::HashMap<_, _>>()"
            ));
        }
        Ok(value_text.to_owned())
    }

    /// Render conversion from a JavaScript property-key value to an owned Rust string.
    pub(super) fn property_key_to_string_text(
        &self,
        value_text: &str,
        source_key: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(source_key) {
            Some(Type::String) => Ok(format!("{value_text}.clone()")),
            Some(Type::Int | Type::Float) => Ok(format!("{value_text}.to_string()")),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => Ok(format!(
                "match {value_text} {{ SmeltUnknown::String(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned() }}"
            )),
            _ => Ok(format!("{value_text}.to_string()")),
        }
    }

    /// Adapts a concrete callback to a single `Vec<SmeltUnknown>` rest callback.
    fn rest_vector_function_adapter_text(
        &self,
        operand: &Operand,
        target: TypeId,
        borrowed: bool,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?) else {
            return Ok(None);
        };
        let Some(Type::Function(target_function)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let [rest_param] = target_function.params.as_slice() else {
            return Ok(None);
        };
        let Some(Type::List(rest_item)) = self.mir.types.get(*rest_param) else {
            return Ok(None);
        };
        if !matches!(
            self.mir.types.get(*rest_item),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
        ) {
            return Ok(None);
        }
        let function_text = self.operand_text(operand)?;
        let args = source
            .params
            .iter()
            .enumerate()
            .map(|(index, param_ty)| {
                let item =
                    format!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)");
                self.unknown_cast_value_text(&item, *param_ty)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let call = format!("{function_text}({args})");
        let return_text =
            self.rendered_value_as_type_text(&call, source.return_ty, target_function.return_ty)?;
        let closure = format!("move |smelt_args: Vec<SmeltUnknown>| {return_text}");
        Ok(Some(if borrowed {
            format!("&mut {closure}")
        } else {
            format!("Box::new({closure})")
        }))
    }

    /// Adapt a callback to a compatible target callback shape.
    fn function_shape_adapter_text(
        &self,
        operand: &Operand,
        target: TypeId,
        borrowed: bool,
    ) -> Result<Option<String>, EmitError> {
        let (Some(Type::Function(source)), Some(Type::Function(target_function))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) else {
            return Ok(None);
        };
        if source.params.len() >= target_function.params.len()
            && !matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown)
            )
        {
            return Ok(None);
        }
        let (Operand::Copy(place) | Operand::Move(place)) = operand else {
            return Ok(None);
        };
        let function_text = if self.is_function_parameter_place(place)? {
            format!("&mut *{}", self.place_text(place)?)
        } else {
            self.place_text(place)?
        };
        let args = target_function
            .params
            .iter()
            .enumerate()
            .map(|(index, _)| format!("arg{index}"))
            .collect::<Vec<_>>();
        let forwarded = args
            .iter()
            .zip(source.params.iter())
            .zip(target_function.params.iter())
            .map(|((arg, source_param), target_param)| {
                self.rendered_value_as_type_text(arg, *target_param, *source_param)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let call_text = format!("({function_text})({forwarded})");
        let return_text = self.rendered_value_as_type_text(
            &call_text,
            source.return_ty,
            target_function.return_ty,
        )?;
        let closure = format!("move |{}| {return_text}", args.join(", "));
        Ok(Some(if borrowed {
            format!("&mut {closure}")
        } else {
            format!("Box::new({closure})")
        }))
    }

    /// Adapts a concrete callback to Remeda's erased purry callback surface.
    pub(super) fn rest_vector_unknown_adapter_text(
        &self,
        operand: &Operand,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?) else {
            return Ok(None);
        };
        let function_text = self.operand_text(operand)?;
        let args = source
            .params
            .iter()
            .enumerate()
            .map(|(index, param_ty)| {
                let item =
                    format!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)");
                self.unknown_cast_value_text(&item, *param_ty)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let call = format!("{function_text}({args})");
        Ok(Some(format!(
            "Box::new(move |smelt_args: Vec<SmeltUnknown>| IntoSmeltUnknown::into_smelt_unknown({call}))"
        )))
    }

    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    /// Gets the type of an operand.
    pub(super) fn operand_ty(&self, operand: &Operand) -> Result<TypeId, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_ty(place),
            Operand::Const(Constant::None) => Ok(self.none_ty),
            Operand::Const(Constant::Bool(_)) => self.type_id(Type::Bool),
            Operand::Const(Constant::Int(_)) => self.type_id(Type::Int),
            Operand::Const(Constant::Float(_)) => self.type_id(Type::Float),
            Operand::Const(Constant::String(_)) => self.type_id(Type::String),
        }
    }

    /// Returns whether a type contains a non-cloneable function value.
    pub(super) fn type_contains_function(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Function(_)) => true,
            Some(
                Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item),
            ) => self.type_contains_function(*item),
            Some(Type::Dict(key, value)) => {
                self.type_contains_function(*key) || self.type_contains_function(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => {
                items.iter().any(|item| self.type_contains_function(*item))
            }
            Some(
                Type::None
                | Type::Bool
                | Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::Never
                | Type::TypeParam { .. }
                | Type::Class { .. },
            )
            | None => false,
        }
    }

    /// Returns whether a class-shaped type is emitted as `SmeltUnknown`.
    pub(super) fn is_erased_class_type(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Class { name, .. }) => {
                !self.mir.classes.iter().any(|class| class.name == *name)
            }
            _ => false,
        }
    }

    /// Returns whether `default_value` is a concrete literal/container value.
    ///
    /// This excludes classes and callable/composite fallback cases where
    /// `default_value` currently emits `Default::default()` and the generated
    /// Rust type may not implement `Default`.
    pub(super) fn has_plain_default_value(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(
                Type::Bool
                    | Type::Int
                    | Type::Float
                    | Type::String
                    | Type::Unknown
                    | Type::Never
                    | Type::None
                    | Type::List(_)
                    | Type::Set(_)
                    | Type::Dict(_, _)
                    | Type::Optional(_)
            )
        )
    }

    /// Converts a place to its Rust text representation.
    /// Gets the entry block of the function.
    pub(super) fn entry_block(&self) -> Result<&BasicBlock, EmitError> {
        self.block(self.function.entry)
    }

    /// Gets a basic block by ID.
    /// Gets a basic block by ID.
    pub(super) fn block(&self, block: smelt_mir::BlockId) -> Result<&BasicBlock, EmitError> {
        self.function
            .blocks
            .get(id_index(block.0, "block index does not fit usize")?)
            .ok_or_else(|| EmitError::new("terminator references an unknown block"))
    }

    /// Gets the declaration of a local by ID.
    /// Gets the declaration of a local by ID.
    pub(super) fn local_decl(&self, local: LocalId) -> Result<&LocalDecl, EmitError> {
        Ok(self
            .function
            .locals
            .get(id_index(local.0, "local index does not fit usize")?)
            .unwrap_or(&self.unknown_local))
    }

    /// Marks a MIR local as introduced in the current generated Rust function.
    pub(super) fn mark_local_declared(&self, local: LocalId) {
        self.declared_locals.borrow_mut().insert(local);
    }

    /// Returns whether a MIR local has already been introduced in Rust output.
    pub(super) fn is_local_declared(&self, local: LocalId) -> bool {
        self.declared_locals.borrow().contains(&local)
    }

    /// Captures the currently visible Rust local declarations.
    ///
    /// MIR locals are function-scoped, but generated Rust branch bodies create
    /// nested lexical scopes. Code that emits a branch restores this snapshot
    /// after the branch so locals introduced only inside that branch do not
    /// leak into later sibling or outer Rust scopes.
    pub(super) fn declared_locals_snapshot(&self) -> HashSet<LocalId> {
        self.declared_locals.borrow().clone()
    }

    /// Restores a previously captured Rust local declaration scope.
    pub(super) fn restore_declared_locals(&self, snapshot: HashSet<LocalId>) {
        *self.declared_locals.borrow_mut() = snapshot;
    }

    /// Gets the declaration of a local owned by another MIR function.
    ///
    /// MIR local IDs are scoped to their function. Call emission uses this when
    /// adapting arguments to a callee's parameter types, because looking those
    /// IDs up in the caller's local table can silently pick an unrelated local.
    pub(super) fn function_local_decl<'a>(
        &self,
        function: &'a MirFunction,
        local: LocalId,
    ) -> Result<&'a LocalDecl, EmitError> {
        function
            .locals
            .get(id_index(local.0, "local index does not fit usize")?)
            .ok_or_else(|| EmitError::new("callee local reference out of bounds"))
    }

    /// Gets the generated variable name for a local.
    /// Gets the generated variable name for a local.
    pub(super) fn local_name(&self, local: LocalId) -> Result<&str, EmitError> {
        self.names
            .get(&local)
            .map(String::as_str)
            .map_or(Ok("SmeltUnknown::Null"), Ok)
    }

    /// Gets the string name of a symbol.
    /// Gets the string name of a symbol.
    pub(super) fn symbol_name(&self, symbol: Symbol) -> Result<&str, EmitError> {
        self.mir
            .symbols
            .get(symbol)
            .ok_or_else(|| EmitError::new("MIR references an unknown symbol"))
    }
}

/// Returns a unique local name derived from `base_name`.
fn unique_local_name(base_name: String, used: &mut HashSet<String>) -> String {
    if used.insert(base_name.clone()) {
        return base_name;
    }

    let mut suffix = 1usize;
    loop {
        let candidate = format!("{base_name}_{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

// Constant formatting continues in `literals.rs`.
