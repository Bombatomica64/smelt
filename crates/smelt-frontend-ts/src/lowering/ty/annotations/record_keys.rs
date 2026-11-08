//! Record key annotations: the key type of `Record<K, V>` and mapped types, and
//! whether a key surface lowers to a string-representable key.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{TSType, TSTypeName};
use smelt_hir::Type;

impl ModuleBuilder<'_> {
    /// Lower a TypeScript `Record<K, V>` key type for Smelt's object model.
    ///
    /// JavaScript object records expose stringified own keys for normal object
    /// operations, so concrete `string`, `number`, literal, `keyof`, and
    /// `PropertyKey`-like key surfaces normalize to `string`. A direct generic
    /// key such as `Record<K, V>` is preserved because later alias substitution
    /// still needs to thread that type parameter through generic helper returns.
    pub(in crate::lowering) fn record_key_type_to_hir(
        &mut self,
        key: &TSType<'_>,
        span: oxc::span::Span,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if let Some(param_ty) = self.direct_record_key_type_param(key)? {
            return Ok(param_ty);
        }
        if self.record_key_surface_is_lowerable(key)? {
            return Ok(self.ctx.krate.types.intern(Type::String));
        }
        Err(SmeltError::unsupported(
            self.span(span.start, span.end),
            "Record keys must lower to TypeScript object property keys",
        ))
    }

    /// Return a direct generic key for `Record<K, V>` without normalizing it.
    ///
    /// Preserving the exact type parameter keeps generic object helpers
    /// instantiable after type aliases are substituted. Composite keys such as
    /// `K | string` use Smelt's string-key object representation instead.
    pub(in crate::lowering) fn direct_record_key_type_param(
        &mut self,
        key: &TSType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        match key {
            TSType::TSParenthesizedType(parenthesized) => {
                self.direct_record_key_type_param(&parenthesized.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if matches!(
                    operator.operator,
                    oxc::ast::ast::TSTypeOperatorOperator::Readonly
                        | oxc::ast::ast::TSTypeOperatorOperator::Unique
                ) =>
            {
                self.direct_record_key_type_param(&operator.type_annotation)
            }
            TSType::TSTypeReference(reference) => {
                let TSTypeName::IdentifierReference(name) = &reference.type_name else {
                    return Ok(None);
                };
                if reference.type_arguments.is_some() {
                    return Ok(None);
                }
                Ok(self.types.param_type(name.name.as_str()))
            }
            _ => Ok(None),
        }
    }

    /// Return whether a `Record` key surface is valid for string-key object lowering.
    ///
    /// TypeScript's checker already enforces that `Record<K, V>` keys are
    /// assignable to `PropertyKey`. This helper keeps Smelt from rejecting
    /// common type-level spellings whose concrete runtime representation is a
    /// JavaScript object with stringified ordinary keys.
    pub(in crate::lowering) fn record_key_surface_is_lowerable(&mut self, key: &TSType<'_>) -> Result<bool, SmeltError> {
        match key {
            TSType::TSStringKeyword(_)
            | TSType::TSNumberKeyword(_)
            | TSType::TSBigIntKeyword(_)
            | TSType::TSSymbolKeyword(_)
            | TSType::TSAnyKeyword(_)
            | TSType::TSUnknownKeyword(_)
            | TSType::TSObjectKeyword(_)
            | TSType::TSTypeQuery(_)
            | TSType::TSTemplateLiteralType(_)
            | TSType::TSNeverKeyword(_) => Ok(true),
            TSType::TSLiteralType(literal) => Ok(matches!(
                literal.literal,
                oxc::ast::ast::TSLiteral::StringLiteral(_)
                    | oxc::ast::ast::TSLiteral::NumericLiteral(_)
                    | oxc::ast::ast::TSLiteral::UnaryExpression(_)
                    | oxc::ast::ast::TSLiteral::BooleanLiteral(_)
            )),
            TSType::TSParenthesizedType(parenthesized) => {
                self.record_key_surface_is_lowerable(&parenthesized.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof =>
            {
                Ok(true)
            }
            TSType::TSTypeOperatorType(operator)
                if matches!(
                    operator.operator,
                    oxc::ast::ast::TSTypeOperatorOperator::Readonly
                        | oxc::ast::ast::TSTypeOperatorOperator::Unique
                ) =>
            {
                self.record_key_surface_is_lowerable(&operator.type_annotation)
            }
            TSType::TSUnionType(union) => union
                .types
                .iter()
                .map(|member| self.record_key_surface_is_lowerable(member))
                .try_fold(true, |all_lowerable, member| {
                    member.map(|member_lowerable| all_lowerable && member_lowerable)
                }),
            TSType::TSIntersectionType(intersection) => intersection
                .types
                .iter()
                .map(|member| self.record_key_surface_is_lowerable(member))
                .try_fold(true, |any_lowerable, member| {
                    member.map(|member_lowerable| any_lowerable || member_lowerable)
                }),
            TSType::TSConditionalType(conditional) => {
                Ok(self.record_key_surface_is_lowerable(&conditional.true_type)?
                    && self.record_key_surface_is_lowerable(&conditional.false_type)?)
            }
            TSType::TSIndexedAccessType(_)
            | TSType::TSMappedType(_)
            | TSType::TSTypeReference(_) => {
                let lowered_key = self.ts_type_to_hir(key)?;
                Ok(self.lowered_record_key_is_string_representable(lowered_key))
            }
            _ => Ok(false),
        }
    }

    /// Return whether an already-lowered key can be represented as object keys.
    pub(in crate::lowering) fn lowered_record_key_is_string_representable(&self, key: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(key)) {
            Some(
                Type::String
                | Type::Int
                | Type::Float
                | Type::Unknown
                | Type::Never
                | Type::TypeParam { .. },
            ) => true,
            Some(Type::Class { name, .. }) => self.ctx.krate.symbols.get(*name).is_some(),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.lowered_record_key_is_string_representable(item)),
            _ => false,
        }
    }

    /// Return whether a `keyof` mapped-type source is entirely array-like.
    pub(in crate::lowering) fn mapped_key_source_is_iterable(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::List(_) | Type::Tuple(_) | Type::Set(_)) => true,
            Some(Type::Union(items)) => {
                !items.is_empty()
                    && items
                        .iter()
                        .copied()
                        .all(|item| self.mapped_key_source_is_iterable(item))
            }
            _ => false,
        }
    }
}
