impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // Type helpers
    // -----------------------------------------------------------------------

    /// Infer the element type of a field access on `receiver_ty`.
    fn field_type(&self, receiver_ty: TypeId, field: Symbol) -> Result<TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Class { name, .. }) => self
                .field_type_on_class(*name, field)
                .or_else(|| self.class_has_no_fields(*name).then_some(receiver_ty))
                .ok_or_else(|| {
                    let field_name = self.ctx.krate.symbols.get(field).unwrap_or("<unknown>");
                    SmeltError::unsupported(
                        Span::new(self.file_id, 0, 0),
                        format!("unknown class field `{field_name}`"),
                    )
                }),
            _ => Err(SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                "attribute access is only supported on class instances",
            )),
        }
    }

    /// Find a field on a class, walking its single base-class chain.
    fn field_type_on_class(&self, class_name: Symbol, field: Symbol) -> Option<TypeId> {
        if let Some(class_name_text) = self.ctx.krate.symbols.get(class_name)
            && let Some(field_ty) = self
                .class_fields
                .get(class_name_text)
                .and_then(|fields| fields.iter().find(|item| item.name == field))
                .map(|item| item.ty)
        {
            return Some(field_ty);
        }
        self.ctx.krate.items.iter().find_map(|item| {
            let Item::Class(class) = item else {
                return None;
            };
            if class.name != class_name {
                return None;
            }
            class
                .descriptors
                .iter()
                .find(|descriptor| descriptor.name == field)
                .map(|descriptor| descriptor.read_ty)
                .or_else(|| {
                    class
                        .fields
                        .iter()
                        .find(|class_field| class_field.name == field)
                        .map(|class_field| class_field.ty)
                })
                .or_else(|| class.base.and_then(|base| self.field_type_on_class(base, field)))
        })
    }

    /// Return whether a class and its bases declare no instance fields.
    fn class_has_no_fields(&self, class_name: Symbol) -> bool {
        self.ctx.krate.items.iter().any(|item| {
            let Item::Class(class) = item else {
                return false;
            };
            class.name == class_name
                && class.fields.is_empty()
                && class.descriptors.is_empty()
                && class.base.is_none_or(|base| {
                    !self.ctx.krate.items.iter().any(|candidate_item| {
                        matches!(candidate_item, Item::Class(base_class) if base_class.name == base)
                    }) || self.class_has_no_fields(base)
                })
        })
    }

    /// Infer the element type of an index access on `receiver_ty`.
    fn index_type(&self, receiver_ty: TypeId) -> Result<TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(elem)) | Some(Type::Set(elem)) => Ok(*elem),
            Some(Type::Dict(_, val)) => Ok(*val),
            Some(Type::String) => Ok(receiver_ty),
            Some(Type::Tuple(items)) => items.first().copied().ok_or_else(|| {
                SmeltError::unsupported(
                    Span::new(self.file_id, 0, 0),
                    "cannot index an empty tuple",
                )
            }),
            _ => Err(SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                "subscript access requires a list, set, dict, tuple, or string",
            )),
        }
    }

    /// Lower a tuple subscript when the index is a static integer literal.
    fn tuple_index_subscript(
        &self,
        tuple_ty: TypeId,
        index_expr: &Expr,
        span: Span,
    ) -> Result<Option<(usize, TypeId)>, SmeltError> {
        let Some(items) = self.ctx.krate.types.get(tuple_ty).and_then(|ty| {
            if let Type::Tuple(items) = ty {
                Some(items.clone())
            } else {
                None
            }
        }) else {
            return Ok(None);
        };
        let Some(raw_index) = Self::static_int_literal(index_expr)? else {
            return Err(SmeltError::unsupported(
                span,
                "tuple indexing requires a static integer index",
            ));
        };
        let index = Self::normalize_tuple_index(raw_index, items.len(), span)?;
        let ty = items[index];
        Ok(Some((index, ty)))
    }

    /// Extract a signed integer literal without lowering it into the HIR body.
    fn static_int_literal(expr: &Expr) -> Result<Option<i64>, SmeltError> {
        match expr {
            Expr::NumberLiteral(number) => match &number.value {
                Number::Int(value) => value.as_i64().map(Some).ok_or_else(|| {
                    SmeltError::unsupported(
                        Span::new(FileId(0), 0, 0),
                        "integer literal out of i64 range",
                    )
                }),
                Number::Float(_) | Number::Complex { .. } => Ok(None),
            },
            Expr::UnaryOp(unary) if unary.op == RuffUnaryOp::USub => {
                let Expr::NumberLiteral(number) = unary.operand.as_ref() else {
                    return Ok(None);
                };
                match &number.value {
                    Number::Int(value) => value
                        .as_i64()
                        .and_then(i64::checked_neg)
                        .map(Some)
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                Span::new(FileId(0), 0, 0),
                                "integer literal out of i64 range",
                            )
                        }),
                    Number::Float(_) | Number::Complex { .. } => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    /// Normalize a Python tuple index and reject out-of-range indexes.
    fn normalize_tuple_index(index: i64, len: usize, span: Span) -> Result<usize, SmeltError> {
        let len_i64 = i64::try_from(len)
            .map_err(|_err| SmeltError::unsupported(span, "tuple length does not fit in i64"))?;
        let normalized = if index < 0 {
            len_i64
                .checked_add(index)
                .ok_or_else(|| SmeltError::unsupported(span, "tuple index is out of range"))?
        } else {
            index
        };
        if normalized < 0 || normalized >= len_i64 {
            return Err(SmeltError::unsupported(span, "tuple index is out of range"));
        }
        usize::try_from(normalized)
            .map_err(|_err| SmeltError::unsupported(span, "tuple index is out of range"))
    }

    /// Infer one item type from a Python iterable type.
    fn iter_item_type(&self, iter_ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(iter_ty) {
            Some(Type::List(elem) | Type::Set(elem)) => Some(*elem),
            Some(Type::Dict(key, _)) => Some(*key),
            Some(Type::Tuple(items)) => items.first().copied(),
            Some(Type::String) => Some(iter_ty),
            _ => None,
        }
    }

    /// Return the indexed element type for fixed-size tuple hints.
    fn tuple_element_type(&self, tuple_ty: TypeId, index: usize) -> Option<TypeId> {
        match self.ctx.krate.types.get(tuple_ty) {
            Some(Type::Tuple(items)) => items.get(index).copied(),
            _ => None,
        }
    }

    /// Return the repeated element type for list hints.
    fn list_element_type(&self, list_ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(list_ty) {
            Some(Type::List(elem) | Type::Set(elem)) => Some(*elem),
            _ => None,
        }
    }

    /// Wrap a Python async return annotation in the shared HIR future type.
    fn future_type(&mut self, inner: TypeId) -> TypeId {
        match self.ctx.krate.types.get(inner) {
            Some(Type::Future(_)) => inner,
            _ => self.intern_type(Type::Future(inner)),
        }
    }

    /// Extract the output type from a HIR future type.
    fn future_inner_type(&self, ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // Built-in items
    // -----------------------------------------------------------------------

    /// Ensure the `print` built-in item exists in the crate and return its id.
    fn ensure_print_item(&mut self, span: Span) -> ItemId {
        if let Some(&id) = self.items.get(smelt_hir::CONSOLE_LOG_SYMBOL) {
            return id;
        }
        let name = self.intern_name(smelt_hir::CONSOLE_LOG_SYMBOL);
        let none_ty = self.intern_type(Type::None);
        let item = Item::Function(Function {
            name,
            span,
            // Synthetic type-helper function carries no type params.
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: None,
return_ty: none_ty,
            is_async: false,
            is_test: false,
            body: None,
            owner: FunctionOwner::Module,
        });
        let id = self.ctx.krate.push_item(item);
        self.items
            .insert(smelt_hir::CONSOLE_LOG_SYMBOL.to_owned(), id);
        id
    }

    // -----------------------------------------------------------------------
    // Interning helpers
    // -----------------------------------------------------------------------

    /// Intern a source-level identifier name.
    fn intern_name(&mut self, name: &str) -> Symbol {
        self.ctx.krate.symbols.intern(name)
    }

    /// Intern an HIR type and return its canonical ID.
    fn intern_type(&mut self, ty: Type) -> TypeId {
        self.ctx.krate.types.intern(ty)
    }

    // -----------------------------------------------------------------------
    // Span helpers
    // -----------------------------------------------------------------------

    /// Convert a Ruff text range to an HIR span in the current file.
    fn span(&self, range: TextRange) -> Span {
        range_to_span(self.file_id, range)
    }
}
