//! Core emission helpers.

use super::*;

impl<'mir> FunctionEmitter<'mir> {
    /// Creates a new function emitter for the given MIR and function.
    pub(crate) fn new(mir: &'mir Mir, function: &'mir MirFunction) -> Result<Self, EmitError> {
        let none_ty = mir
            .types
            .all()
            .iter()
            .enumerate()
            .find_map(|(id, ty)| {
                (*ty == Type::None)
                    .then(|| compact_index(id, "type index does not fit u32").map(TypeId))
            })
            .transpose()?
            .ok_or_else(|| EmitError::new("MIR is missing the None type"))?;
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
        Ok(Self {
            mir,
            function,
            names,
            mutable_locals: assigned_locals(mir, function),
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
                LocalKind::Param => {
                    if matches!(function.origin, HirOrigin::ClassMethod { .. })
                        && function.params.first() == Some(&local_id)
                    {
                        "self".to_owned()
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
                        self.type_text(local.ty)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            out.push_str(&format!(
                "fn {}({fn_params}) -> {} {{\n",
                self.function_rust_name(self.function)?,
                self.return_type_text(self.function.return_ty)?
            ));
            out.push_str(
                "    let _ = &mut arg_0;\n    move |_item, _index, _items| SmeltUnknown::Null\n}\n",
            );
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
                    let mutability =
                        if matches!(self.mir.types.get(local.ty), Some(Type::Function(_))) {
                            "mut "
                        } else {
                            ""
                        };
                    Ok(format!(
                        "{mutability}{}: {}",
                        self.local_name(*param)?,
                        self.type_text(local.ty)?
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

        self.emit_block(self.entry_block()?, out)?;
        out.push_str("}\n");
        Ok(())
    }

    /// Return the emitted Rust name for a free MIR function.
    ///
    /// Source modules can contain same-named local helper functions. Because
    /// the current backend emits one flat Rust module, duplicate source names
    /// are disambiguated with the MIR function id while unique public names
    /// keep their readable spelling.
    pub(super) fn function_rust_name(&self, function: &MirFunction) -> Result<String, EmitError> {
        let source_name = self.symbol_name(function.name)?;
        let base = sanitize_ident(source_name);
        if !function.is_test && source_name == "main" && function.return_ty == self.none_ty {
            return Ok(base);
        }
        let same_name_count = self
            .mir
            .functions
            .iter()
            .filter(|candidate| {
                candidate.name == function.name
                    && !matches!(
                        candidate.origin,
                        HirOrigin::ClassConstructor { .. } | HirOrigin::ClassMethod { .. }
                    )
            })
            .count();
        if same_name_count > 1 || source_name.starts_with("__smelt_module_") {
            Ok(format!("{}_{}", base, function.id.0))
        } else {
            Ok(base)
        }
    }

    /// Return the emitted Rust name for a callback function symbol.
    pub(super) fn callback_function_rust_name(
        &self,
        function: Symbol,
    ) -> Result<String, EmitError> {
        if let Some(candidate) = self
            .mir
            .functions
            .iter()
            .find(|candidate| candidate.name == function)
        {
            return self.function_rust_name(candidate);
        }
        Ok(sanitize_ident(self.symbol_name(function)?))
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
                        let mutability =
                            if matches!(self.mir.types.get(local.ty), Some(Type::Function(_))) {
                                "mut "
                            } else {
                                ""
                            };
                        Ok(format!(
                            "{mutability}{}: {}",
                            self.local_name(*param)?,
                            self.type_text(local.ty)?
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
                            self.type_text(local.ty)?
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

    /// Converts a type ID to its Rust text representation.
    /// Converts a type ID to its Rust text representation.
    pub(crate) fn type_text_for(mir: &Mir, ty: TypeId) -> Result<String, EmitError> {
        FunctionEmitter {
            mir,
            function: mir
                .functions
                .first()
                .ok_or_else(|| EmitError::new("MIR has no functions"))?,
            names: HashMap::new(),
            mutable_locals: HashSet::new(),
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
        .type_text(ty)
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
        if self.mir.types.get(target) == Some(&Type::Unknown) {
            return self.unknown_wrap_text(operand);
        }
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) {
            return self.unknown_cast_text(operand, target);
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Function(_))
            )
        {
            if let Some(adapter) = self.rest_vector_function_adapter_text(operand, target)? {
                return Ok(adapter);
            }
            return Ok(format!("Box::new({})", self.operand_text(operand)?));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            let operand_ty = self.operand_ty(operand)?;
            if self.mir.types.get(operand_ty) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if operand_ty == *inner {
                return Ok(format!("Some({})", self.operand_text(operand)?));
            }
        }
        if let (Some(Type::Function(source)), Some(Type::Function(target_function))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && (source.params.len() < target_function.params.len()
            || matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown)
            ))
            && let Operand::Copy(place) | Operand::Move(place) = operand
        {
            let function_text = self.place_text(place)?;
            let args = target_function
                .params
                .iter()
                .enumerate()
                .map(|(index, _)| format!("arg{index}"))
                .collect::<Vec<_>>();
            let forwarded = args
                .iter()
                .take(source.params.len())
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let call_text = format!("{function_text}({forwarded})");
            let return_text = if matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown)
            ) {
                format!("IntoSmeltUnknown::into_smelt_unknown({call_text})")
            } else {
                call_text
            };
            return Ok(format!("move |{}| {return_text}", args.join(", "),));
        }
        self.operand_text(operand)
    }

    /// Adapts a concrete callback to a single `Vec<SmeltUnknown>` rest callback.
    fn rest_vector_function_adapter_text(
        &self,
        operand: &Operand,
        target: TypeId,
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
        if self.mir.types.get(*rest_item) != Some(&Type::Unknown) {
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
        let return_text = if self.mir.types.get(target_function.return_ty) == Some(&Type::Unknown) {
            format!("IntoSmeltUnknown::into_smelt_unknown({call})")
        } else {
            call
        };
        Ok(Some(format!("Box::new(move |smelt_args| {return_text})")))
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
