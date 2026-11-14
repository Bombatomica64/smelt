//! Array-constrained function type parameters, carried as their element type.
//!
//! # The rule
//!
//! A function type parameter whose constraint lowers to a list
//! (`T extends readonly unknown[]`, `T extends ReadonlyArray<X> | readonly []`,
//! `T extends number[]`) is generic over its ELEMENT: inside the declaration
//! the name `T` resolves to `List(T)`, and the parameter symbol `T` itself
//! stands for the element, constrained by the element of the declared
//! constraint (none for `unknown[]`: an `unknown` bound admits every type).
//! An element bounded by a concrete type (`T extends readonly number[]`) is
//! read as that type instead (`List(number)`), as a hand port writes it. `T[number]` then reads the element off the list like any
//! indexed access over an array (`ty/annotations.rs`), and `{ [K in keyof T]: F<T[K]> }`
//! is a list of `F<element>`.
//!
//! This is the signature a hand port writes: remeda's
//! `dropWhile<T extends IterableContainer>(data: T, predicate: (item: T[number], ..) => boolean): T[number][]`
//! is `fn drop_while<T>(data: Vec<T>, predicate: impl Fn(&T, ..) -> bool) -> Vec<T>`.
//! Without the rule `T[number]` is a type-level operator over a free
//! parameter, which neither Smelt nor the checker can evaluate at the
//! declaration, so it erased, and with it every value read out of `data`.
//!
//! # Requirements
//!
//! - Behaviour: none of the runtime operations change. A value of `T` is an
//!   array at run time whatever `T` is instantiated to, because its
//!   constraint says so, and the lowering already represents arrays and the
//!   tuples a caller may pass as lists of their element.
//! - Type preservation: `T[number]` keeps the element type each call
//!   instantiates (`number` for `dropWhile([1, 2], ..)`) instead of
//!   `unknown`; `T` keeps "list of that element".
//!
//! # Scope
//!
//! Function-like declarations and type aliases take the rule (see
//! [`ModuleBuilder::push_alias_type_parameter_scope`] for which aliases).
//!
//! A parameter that types a rest parameter or is spread into a tuple anywhere
//! in the signature (`<A extends any[]>(f: (...args: A) => R)`,
//! `zipped: ReadonlyArray<[...T]>`) is not carried: there it is a positional
//! TUPLE (an argument list, a row), whose positions have their own types, and
//! one element type would union them.
//!
//! A function's type parameters are bound per call by inference, which reads
//! the element straight off a list argument; an alias's by explicit
//! arguments. An explicit type argument for a carried parameter names the
//! whole list, so [`ModuleBuilder::type_argument_substitution`] peels it back
//! to the element (`HirCtx::element_carried_type_params`). Classes and
//! interfaces keep the plain scope: their instances carry their arguments
//! into struct layouts and constructor inference that read them unpeeled.

use std::collections::{HashMap, HashSet};

use oxc::ast::ast::{
    ArrowFunctionExpression, FormalParameters, Function, TSRestType, TSType, TSTypeAnnotation,
    TSTypeName, TSTypeParameterDeclaration,
};
use oxc::ast_visit::{Visit, walk};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use smelt_hir::{ErasureReason, Type, TypeId, TypeParamDef};

/// The value parameters and return annotation of a function-like declaration,
/// read to find the type parameters that type a rest parameter.
#[derive(Clone, Copy)]
pub(in crate::lowering) struct FunctionSignature<'s, 'a> {
    /// The declaration's own value parameters.
    pub(in crate::lowering) params: &'s FormalParameters<'a>,
    /// The declared return annotation, if any.
    pub(in crate::lowering) return_type: Option<&'s TSTypeAnnotation<'a>>,
}

impl<'s, 'a> FunctionSignature<'s, 'a> {
    /// The signature of a function declaration or expression.
    pub(in crate::lowering) fn of_function(function: &'s Function<'a>) -> Self {
        Self {
            params: &function.params,
            return_type: function.return_type.as_deref(),
        }
    }

    /// The signature of an arrow function.
    pub(in crate::lowering) fn of_arrow(arrow: &'s ArrowFunctionExpression<'a>) -> Self {
        Self {
            params: &arrow.params,
            return_type: arrow.return_type.as_deref(),
        }
    }

    /// Names of the bare type references that annotate a rest parameter or
    /// are spread into a tuple type (`[...T]`) in this signature, its
    /// function-typed parameters and return included.
    fn rest_parameter_type_names(self) -> HashSet<String> {
        let mut finder = RestParameterTypes::default();
        finder.visit_formal_parameters(self.params);
        if let Some(return_type) = self.return_type {
            finder.visit_ts_type_annotation(return_type);
        }
        finder.names
    }
}

/// Collects the names of the bare type references that annotate rest
/// parameters or are spread into tuple types: the type parameters used as
/// positional tuples.
#[derive(Default)]
struct RestParameterTypes {
    /// The names found so far.
    names: HashSet<String>,
}

impl RestParameterTypes {
    /// Record `ty` when it is a bare reference to a name.
    fn record_bare_reference(&mut self, ty: &TSType<'_>) {
        if let TSType::TSTypeReference(reference) = ty
            && reference.type_arguments.is_none()
            && let TSTypeName::IdentifierReference(name) = &reference.type_name
        {
            self.names.insert(name.name.to_string());
        }
    }
}

impl<'a> Visit<'a> for RestParameterTypes {
    fn visit_formal_parameters(&mut self, it: &FormalParameters<'a>) {
        if let Some(rest) = &it.rest
            && let Some(annotation) = &rest.type_annotation
        {
            self.record_bare_reference(&annotation.type_annotation);
        }
        walk::walk_formal_parameters(self, it);
    }

    fn visit_ts_rest_type(&mut self, it: &TSRestType<'a>) {
        self.record_bare_reference(&it.type_annotation);
        walk::walk_ts_rest_type(self, it);
    }
}

impl ModuleBuilder<'_> {
    /// Push the type-parameter scope of a FUNCTION-LIKE declaration, carrying
    /// every array-constrained parameter as its element (see the module docs).
    ///
    /// The declaration is first pushed as an ordinary scope, which decides
    /// which constraints are lists; when one is, that scope and the erasure
    /// records its constraints made are replaced by the carrying scope, so
    /// every parameter's bounds lower once, against the final meaning of its
    /// siblings. Pop it with [`Self::pop_type_parameter_scope`].
    pub(in crate::lowering) fn push_function_type_parameter_scope(
        &mut self,
        params: Option<&TSTypeParameterDeclaration<'_>>,
        signature: FunctionSignature<'_, '_>,
    ) -> Result<Vec<TypeParamDef>, SmeltError> {
        self.push_element_carrying_scope(params, &signature.rest_parameter_type_names())
    }

    /// Push the type-parameter scope of a type alias whose body is `body`,
    /// carrying every array-constrained parameter as its element.
    ///
    /// An alias is instantiated by explicit arguments wherever it is named,
    /// which [`Self::explicit_type_argument_binding`] peels to the element,
    /// so its body may read `T[number]` as the element as a function does.
    /// The caller keeps the plain scope for an alias that becomes an
    /// interface: those share their parameters with the class-default
    /// prepass (`collect_class_type_parameter_defaults`).
    pub(in crate::lowering) fn push_alias_type_parameter_scope(
        &mut self,
        params: Option<&TSTypeParameterDeclaration<'_>>,
        body: &TSType<'_>,
    ) -> Result<Vec<TypeParamDef>, SmeltError> {
        let mut finder = RestParameterTypes::default();
        finder.visit_ts_type(body);
        self.push_element_carrying_scope(params, &finder.names)
    }

    /// Push `params`, carrying every array-constrained parameter not named in
    /// `argument_lists` as its element (see
    /// [`Self::push_function_type_parameter_scope`]).
    fn push_element_carrying_scope(
        &mut self,
        params: Option<&TSTypeParameterDeclaration<'_>>,
        argument_lists: &HashSet<String>,
    ) -> Result<Vec<TypeParamDef>, SmeltError> {
        let records_before = self.ctx.krate.erasures.len();
        let defs = self.push_type_parameter_scope(params)?;
        let Some(params) = params else {
            return Ok(defs);
        };
        let carried: HashMap<String, Option<TypeId>> = params
            .params
            .iter()
            .zip(&defs)
            .filter(|(param, _)| !argument_lists.contains(param.name.name.as_str()))
            .filter_map(|(param, def)| {
                let element = self.list_element_type(def.constraint?)?;
                Some((
                    param.name.name.to_string(),
                    self.concrete_element_bound(element),
                ))
            })
            .collect();
        if carried.is_empty() {
            return Ok(defs);
        }
        self.pop_type_parameter_scope();
        self.ctx.krate.erasures.truncate(records_before);
        let carrying_defs = self.push_type_parameter_scope_carrying(Some(params), &carried)?;
        for (param, def) in params.params.iter().zip(&carrying_defs) {
            if carried.contains_key(param.name.name.as_str()) {
                self.ctx.element_carried_type_params.insert(def.name);
            }
        }
        Ok(carrying_defs)
    }

    /// [`Self::with_type_parameter_scope`] for a function-like declaration:
    /// the scope is pushed by [`Self::push_function_type_parameter_scope`] and
    /// popped on every exit path of `lower`.
    pub(in crate::lowering) fn with_function_type_parameter_scope<R>(
        &mut self,
        params: Option<&TSTypeParameterDeclaration<'_>>,
        signature: FunctionSignature<'_, '_>,
        lower: impl FnOnce(&mut Self, Vec<TypeParamDef>) -> Result<R, SmeltError>,
    ) -> Result<R, SmeltError> {
        let defs = self.push_function_type_parameter_scope(params, signature)?;
        let result = lower(self, defs);
        self.pop_type_parameter_scope();
        result
    }

    /// The element bound a carried parameter is read AS, when it has one.
    ///
    /// An element bounded by a concrete type (`T extends readonly number[]`)
    /// is that type: a hand port writes `Vec<f64>`, and a type parameter
    /// bounded by `number` would only reach Rust as the erased value (codegen
    /// carries unbounded parameters as generics). An element with no bound
    /// (`unknown[]`), a bound naming another parameter, or a bound that is a
    /// union of shapes (`PropertyKey`, `string | number`, an optional) stays
    /// a generic parameter: each call then binds the member it uses (`["a"]`
    /// binds `string`), where the union read as the element would make every
    /// value the union. A `string` bound stays generic too: Smelt lowers
    /// `keyof T` to `string`, a narrowing of the key type `string | number |
    /// symbol` (remeda's `Keys extends readonly (keyof T)[]` holds symbol
    /// keys), so a string element bound may stand for property keys.
    fn concrete_element_bound(&self, element: TypeId) -> Option<TypeId> {
        let single_shape = !matches!(
            self.ctx.krate.types.get(element),
            Some(Type::Unknown | Type::Union(_) | Type::Optional(_) | Type::PropKey | Type::String)
        );
        (single_shape && !self.type_mentions_any_param(element)).then_some(element)
    }

    /// Whether `ty` mentions a type parameter anywhere.
    fn type_mentions_any_param(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::TypeParam { .. }) => true,
            Some(other) => other
                .children()
                .any(|child| self.type_mentions_any_param(child)),
            None => false,
        }
    }

    /// The element type of `ty` when it is a list.
    fn list_element_type(&self, ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::List(element)) => Some(*element),
            _ => None,
        }
    }

    /// For a `carried` parameter, the element of its declared bound or
    /// default `ty`; `ty` itself otherwise.
    ///
    /// The ordinary pass already established that a carried parameter's
    /// constraint is a list, so the element always exists for it; a default
    /// that is not a list (`= []` lowers to one) keeps its lowered type.
    pub(in crate::lowering) fn carried_element_or_self(&self, ty: TypeId, carried: bool) -> TypeId {
        if carried {
            self.list_element_type(ty).unwrap_or(ty)
        } else {
            ty
        }
    }

    /// The binding of `param` for an explicit type argument `actual`.
    ///
    /// A carried parameter stands for the element of the list the source
    /// spells, so `f<number[]>(..)` binds it to `number`, and a tuple argument
    /// binds the union of its items, which is what TypeScript reads as the
    /// tuple's `[number]` (`never` for `[]`). That loses only the positions,
    /// which a list of the parameter does not keep in the body either. A type
    /// parameter argument is read through its constraint (the element of a
    /// parameter that is itself a tuple is no single type a hand port could
    /// name), and an argument that is no array at all has no element.
    pub(in crate::lowering) fn explicit_type_argument_binding(
        &mut self,
        param: &TypeParamDef,
        actual: TypeId,
    ) -> TypeId {
        if !self.ctx.element_carried_type_params.contains(&param.name) {
            return actual;
        }
        let argument = self.type_param_constraint_or_self(actual);
        match self.ctx.krate.types.get(argument).cloned() {
            Some(Type::List(element)) => element,
            Some(Type::Tuple(items)) => {
                let mut members = Vec::new();
                for item in items {
                    for member in self.flatten_union_member_types(item) {
                        if !members.contains(&member) {
                            members.push(member);
                        }
                    }
                }
                if members.is_empty() {
                    self.ctx.krate.types.intern(Type::Never)
                } else {
                    self.union_of_types_or_unknown(members)
                }
            }
            // not a new erasure: the argument itself is already erased
            Some(Type::Unknown) => self.unknown_type(NotNewErasure::ErasedUpstream),
            _ => self.erase_with(
                ErasureReason::TypeLevel,
                "non-array argument for an element-carried type parameter",
            ),
        }
    }
}
