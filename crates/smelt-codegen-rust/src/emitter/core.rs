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
        let names = Self::local_names(mir, function)?;
        Ok(Self {
            mir,
            function,
            names,
            mutable_locals: assigned_locals(mir, function),
            none_ty,
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
                sanitize_ident(name),
                self.return_type_text(self.function.return_ty)?
            ));
        }

        self.emit_block(self.entry_block()?, out)?;
        out.push_str("}\n");
        Ok(())
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
                if matches!(
                    self.mir.types.get(self.place_ty(place)?),
                    Some(Type::Function(_))
                ) {
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
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            let operand_ty = self.operand_ty(operand)?;
            if self.mir.types.get(operand_ty) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if operand_ty == *inner {
                return Ok(format!("Some({})", self.operand_text(operand)?));
            }
        }
        self.operand_text(operand)
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
    pub(super) fn local_decl(&self, local: LocalId) -> Result<&smelt_mir::LocalDecl, EmitError> {
        self.function
            .locals
            .get(id_index(local.0, "local index does not fit usize")?)
            .ok_or_else(|| EmitError::new("MIR references an unknown local"))
    }

    /// Gets the generated variable name for a local.
    /// Gets the generated variable name for a local.
    pub(super) fn local_name(&self, local: LocalId) -> Result<&str, EmitError> {
        self.names
            .get(&local)
            .map(String::as_str)
            .ok_or_else(|| EmitError::new("MIR references an unnamed local"))
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
