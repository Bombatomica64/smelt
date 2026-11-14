//! Virtual method fields: methods a subclass overrides or a constructor assigns
//! on `this`, carried as function-typed fields so dispatch reaches the override.

use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{
    ArrayExpressionElement, ClassElement, Expression, MethodDefinitionKind, MethodDefinitionType,
    ObjectPropertyKind, PropertyKey, Statement,
};
use smelt_hir::{Field, FunctionType, Item, MethodSig, Type};
use std::borrow::Cow;
use std::collections::HashSet;

impl ModuleBuilder<'_> {
    /// Add callable storage slots for a class's virtual method surface.
    ///
    /// TypeScript lets concrete subclass instances flow through base-class
    /// references such as `Parser<any>`. Generated Rust needs stored callable
    /// slots so structural adapters can bind the concrete implementation
    /// instead of erasing external `baseRef.method()` calls or `this.method`
    /// reads to the abstract/base default.
    pub(in crate::lowering) fn add_virtual_class_method_fields(
        &mut self,
        fields: &mut Vec<Field>,
        methods: &[MethodSig],
        virtual_method_fields: &HashSet<smelt_hir::Symbol>,
    ) {
        let new_fields = self.virtual_class_method_fields(fields, methods, virtual_method_fields);
        fields.extend(new_fields);
    }

    /// Build callable storage slots for selected virtual class methods.
    pub(in crate::lowering) fn virtual_class_method_fields(
        &mut self,
        existing_fields: &[Field],
        methods: &[MethodSig],
        virtual_method_fields: &HashSet<smelt_hir::Symbol>,
    ) -> Vec<Field> {
        let mut fields = Vec::new();
        for method in methods {
            if !virtual_method_fields.contains(&method.name) {
                continue;
            }
            if existing_fields
                .iter()
                .chain(fields.iter())
                .any(|field| field.name == method.name)
            {
                continue;
            }
            let params = method
                .params
                .iter()
                .map(|param| param.ty)
                .collect::<Vec<_>>();
            let mutable_params =
                self.mutable_params_from_returned_tuple_state(&params, method.return_ty);
            let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params,
                rest: method.rest,
                required_params: method.required_params,
                mutable_params,
                return_ty: method.return_ty,
                is_async: method.is_async,
                may_throw: false,
            }));
            fields.push(Field {
                name: method.name,
                ty,
                visibility: method.visibility,
                optional: false,
                span: method.span,
            });
        }
        fields
    }

    /// Add callable slots to a base class when a subclass overrides its method.
    ///
    /// A value stored as the base type must keep the concrete override for
    /// later `baseRef.method()` calls. This pass runs when the subclass is
    /// seen, because that is the point where override usage is known.
    pub(in crate::lowering) fn add_overridden_base_method_fields(
        &mut self,
        base: Option<smelt_hir::Symbol>,
        subclass_methods: &[MethodSig],
    ) {
        let Some(base) = base else {
            return;
        };
        let Some(base_name) = self.ctx.krate.symbols.get(base).map(str::to_owned) else {
            return;
        };
        let Some(base_methods) = self.classes.methods(&base_name).cloned() else {
            return;
        };
        let base_method_names = base_methods
            .iter()
            .map(|method| method.name)
            .collect::<HashSet<_>>();
        let overridden_methods = subclass_methods
            .iter()
            .filter_map(|method| {
                base_method_names
                    .contains(&method.name)
                    .then_some(method.name)
            })
            .collect::<HashSet<_>>();
        if overridden_methods.is_empty() {
            return;
        }

        let existing_fields = self.classes.fields(&base_name).cloned().unwrap_or_default();
        let new_fields =
            self.virtual_class_method_fields(&existing_fields, &base_methods, &overridden_methods);
        if new_fields.is_empty() {
            return;
        }
        self.classes.extend_fields(base_name, new_fields.clone());
        if let Some(Item::Class(class)) = self
            .ctx
            .krate
            .items
            .iter_mut()
            .find(|item| matches!(item, Item::Class(class) if class.name == base))
        {
            class.fields.extend(new_fields);
        }
    }

    /// Collect method names that require callable storage on class instances.
    ///
    /// Abstract method declarations are always virtual from the point of view
    /// of erased base-class references. Concrete methods only need storage when
    /// source reads them as values through `this.<method>`.
    pub(in crate::lowering) fn virtual_method_field_names(
        &mut self,
        elements: &[ClassElement<'_>],
        include_abstract_methods: bool,
    ) -> HashSet<smelt_hir::Symbol> {
        let mut names = HashSet::new();
        for element in elements {
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            if include_abstract_methods
                && method.r#type == MethodDefinitionType::TSAbstractMethodDefinition
                && method.kind == MethodDefinitionKind::Method
            {
                let name = match &method.key {
                    PropertyKey::StaticIdentifier(identifier) => {
                        Some(Cow::Borrowed(identifier.name.as_str()))
                    }
                    PropertyKey::PrivateIdentifier(identifier) => {
                        Some(Cow::Borrowed(identifier.name.as_str()))
                    }
                    PropertyKey::StringLiteral(literal) => Some(js_str_text(literal.value)),
                    _ => None,
                };
                if let Some(name) = name {
                    names.insert(self.intern_source_name(&name));
                }
            }
            if include_abstract_methods {
                let Some(body) = &method.value.body else {
                    continue;
                };
                for statement in &body.statements {
                    self.collect_this_member_names_from_statement(statement, &mut names);
                }
            }
        }
        names
    }

    /// Collect `this.<name>` member reads from a statement subtree.
    pub(in crate::lowering) fn collect_this_member_names_from_statement(
        &mut self,
        statement: &Statement<'_>,
        names: &mut HashSet<smelt_hir::Symbol>,
    ) {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    if let Some(init) = &declarator.init {
                        self.collect_this_member_names_from_expression(init, names);
                    }
                }
            }
            Statement::ExpressionStatement(statement) => {
                self.collect_this_member_names_from_expression(&statement.expression, names);
            }
            Statement::ReturnStatement(statement) => {
                if let Some(argument) = &statement.argument {
                    self.collect_this_member_names_from_expression(argument, names);
                }
            }
            Statement::IfStatement(statement) => {
                self.collect_this_member_names_from_expression(&statement.test, names);
                self.collect_this_member_names_from_statement(&statement.consequent, names);
                if let Some(alternate) = &statement.alternate {
                    self.collect_this_member_names_from_statement(alternate, names);
                }
            }
            Statement::BlockStatement(block) => {
                for block_statement in &block.body {
                    self.collect_this_member_names_from_statement(block_statement, names);
                }
            }
            _ => {}
        }
    }

    /// Collect `this.<name>` member reads from an expression subtree.
    pub(in crate::lowering) fn collect_this_member_names_from_expression(
        &mut self,
        expression: &Expression<'_>,
        names: &mut HashSet<smelt_hir::Symbol>,
    ) {
        match expression {
            Expression::StaticMemberExpression(member) => {
                if matches!(&member.object, Expression::ThisExpression(_)) {
                    names.insert(self.intern_source_name(member.property.name.as_str()));
                }
                self.collect_this_member_names_from_expression(&member.object, names);
            }
            Expression::CallExpression(call) => {
                self.collect_this_member_names_from_expression(&call.callee, names);
                for argument in &call.arguments {
                    if let Some(argument_expression) = argument.as_expression() {
                        self.collect_this_member_names_from_expression(argument_expression, names);
                    }
                }
            }
            Expression::NewExpression(new_expr) => {
                self.collect_this_member_names_from_expression(&new_expr.callee, names);
                for argument in &new_expr.arguments {
                    if let Some(argument_expression) = argument.as_expression() {
                        self.collect_this_member_names_from_expression(argument_expression, names);
                    }
                }
            }
            Expression::BinaryExpression(binary) => {
                self.collect_this_member_names_from_expression(&binary.left, names);
                self.collect_this_member_names_from_expression(&binary.right, names);
            }
            Expression::LogicalExpression(logical) => {
                self.collect_this_member_names_from_expression(&logical.left, names);
                self.collect_this_member_names_from_expression(&logical.right, names);
            }
            Expression::UnaryExpression(unary) => {
                self.collect_this_member_names_from_expression(&unary.argument, names);
            }
            Expression::ConditionalExpression(conditional) => {
                self.collect_this_member_names_from_expression(&conditional.test, names);
                self.collect_this_member_names_from_expression(&conditional.consequent, names);
                self.collect_this_member_names_from_expression(&conditional.alternate, names);
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.collect_this_member_names_from_expression(&parenthesized.expression, names);
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            self.collect_this_member_names_from_expression(&property.value, names);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.collect_this_member_names_from_expression(&spread.argument, names);
                        }
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(spread) => {
                            self.collect_this_member_names_from_expression(&spread.argument, names);
                        }
                        other => {
                            if let Some(element_expression) = other.as_expression() {
                                self.collect_this_member_names_from_expression(
                                    element_expression,
                                    names,
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
