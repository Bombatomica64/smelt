impl ModuleBuilder<'_> {
    /// Convert a TypeScript property key into the interned HIR symbol it names.
    fn property_key_symbol(
        &mut self,
        key: &PropertyKey<'_>,
    ) -> Result<smelt_hir::Symbol, SmeltError> {
        match key {
            PropertyKey::StaticIdentifier(ident) => {
                Ok(self.intern_source_name(ident.name.as_str()))
            }
            PropertyKey::PrivateIdentifier(ident) => {
                Ok(self.intern_source_name(ident.name.as_str()))
            }
            PropertyKey::StringLiteral(lit) => Ok(self.intern_source_name(lit.value.as_str())),
            _ => Err(SmeltError::unsupported(
                self.span(key.span().start, key.span().end),
                "property names must be static identifiers or string literals",
            )),
        }
    }

    /// Resolve a class `implements` clause entry to an interface symbol.
    fn implements_symbol(
        &mut self,
        item: &oxc::ast::ast::TSClassImplements<'_>,
    ) -> Result<smelt_hir::Symbol, SmeltError> {
        if item.type_arguments.is_some() {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "generic implements clauses are not lowered yet",
            ));
        }
        let TSTypeName::IdentifierReference(name) = &item.expression else {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "qualified implements clauses are not lowered yet",
            ));
        };
        Ok(self.intern_type_name(name.name.as_str()))
    }

    /// Convert an interface heritage clause to the referenced interface symbol and arguments.
    fn interface_heritage(
        &mut self,
        item: &oxc::ast::ast::TSInterfaceHeritage<'_>,
    ) -> Result<(smelt_hir::Symbol, Vec<smelt_hir::TypeId>), SmeltError> {
        let Expression::Identifier(name) = &item.expression else {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "qualified interface inheritance is not lowered yet",
            ));
        };
        let args = item
            .type_arguments
            .as_ref()
            .map(|args| {
                args.params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok((self.intern_type_name(name.name.as_str()), args))
    }

    /// Find a previously lowered interface by symbol.
    fn find_interface(&self, name: smelt_hir::Symbol) -> Option<&Interface> {
        self.ctx.krate.items.iter().find_map(|item| {
            if let Item::Interface(interface) = item {
                if interface.name == name {
                    return Some(interface);
                }
            }
            None
        })
    }

    /// Find a previously lowered type alias by symbol.
    fn find_type_alias(&self, name: smelt_hir::Symbol) -> Option<&smelt_hir::TypeAlias> {
        self.ctx.krate.items.iter().find_map(|item| {
            if let Item::TypeAlias(alias) = item
                && alias.name == name
            {
                return Some(alias);
            }
            None
        })
    }

    /// Validate that a lowered class satisfies all declared interfaces.
    fn validate_implements(&self, class_item: smelt_hir::ItemId) -> Result<(), SmeltError> {
        let Item::Class(class) = self.item_ref(class_item) else {
            return Ok(());
        };
        for interface_name in &class.implements {
            let interface = self
                .ctx
                .krate
                .items
                .iter()
                .find_map(|item| {
                    if let Item::Interface(interface) = item
                        && interface.name == *interface_name
                    {
                        return Some(interface);
                    }
                    None
                })
                .ok_or_else(|| {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(*interface_name)
                        .unwrap_or("<unknown>");
                    SmeltError::unsupported(
                        class.span,
                        format!("implemented interface `{name}` is not declared"),
                    )
                })?;
            for required in &interface.fields {
                let Some(actual) = class
                    .fields
                    .iter()
                    .find(|field| field.name == required.name)
                else {
                    if required.optional {
                        continue;
                    }
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        required.span,
                        format!("class is missing implemented interface field `{name}`"),
                    ));
                };
                if !field_type_satisfies(&self.ctx.krate, actual.ty, required) {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        actual.span,
                        format!("implemented interface field `{name}` has a mismatched type"),
                    ));
                }
            }
            for required in &interface.methods {
                let Some(actual_item) = class.methods.iter().find(|method_item| {
                    matches!(self.item_ref(**method_item), Item::Function(function) if function.name == required.name)
                }) else {
                    let name = self.ctx.krate.symbols.get(required.name).unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(required.span, format!("class is missing implemented interface method `{name}`")));
                };
                let Item::Function(actual) = self.item_ref(*actual_item) else {
                    return Err(SmeltError::unsupported(
                        required.span,
                        "implemented interface method has an unexpected item kind",
                    ));
                };
                let actual_params = actual
                    .params
                    .iter()
                    .filter(|param| self.ctx.krate.symbols.get(param.name) != Some("this"))
                    .map(|param| param.ty)
                    .collect::<Vec<_>>();
                let required_params = required
                    .params
                    .iter()
                    .map(|param| param.ty)
                    .collect::<Vec<_>>();
                if actual_params != required_params || actual.return_ty != required.return_ty {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        actual.span,
                        format!("implemented interface method `{name}` has a mismatched signature"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Look up the type of an existing expression.
    fn expr_ty(body: &Body, expr: smelt_hir::ExprId) -> smelt_hir::TypeId {
        let index = usize::try_from(expr.0).expect("expr id should fit into usize");
        body.exprs
            .get(index)
            .expect("expr id should point to an existing expression")
            .ty
    }

    /// Look up the type of an existing local.
    fn local_ty(body: &Body, local: smelt_hir::LocalId) -> smelt_hir::TypeId {
        let index = usize::try_from(local.0).expect("local id should fit into usize");
        body.locals
            .get(index)
            .expect("local id should point to an existing local")
            .ty
    }

    /// Look up a lowered item by id.
    fn item_ref(&self, item: smelt_hir::ItemId) -> &Item {
        let index = usize::try_from(item.0).expect("item id should fit into usize");
        self.ctx
            .krate
            .items
            .get(index)
            .expect("item id should point to an existing item")
    }
}
