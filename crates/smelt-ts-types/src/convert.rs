//! Conversion of tsrs checker types into the owned [`crate::model`].
//!
//! The converter walks a checker `Type` and its constituents once, interning
//! each tsrs type by pointer identity so a type shared by many expressions (or
//! reached recursively through its own members) gets a single [`TsTypeId`].
//! A slot is reserved *before* a type's members are converted, which is what
//! makes self-referential types terminate.
//!
//! Classification follows TypeScript's own flag order: intrinsic and literal
//! flags first, then unions (where `boolean` and enum types live), then object
//! types split into arrays, tuples, nominal class/interface references and
//! structural objects. Only structural objects are expanded member by member;
//! nominal types are referenced by symbol plus type arguments, as a hand-ported
//! Rust crate would reference a struct by name. Anonymous types that keep
//! instantiating new anonymous types (`type F<T> = { next: F<[T]> }`) stop at
//! [`MAX_STRUCTURAL_DEPTH`] with [`TsTypeKind::Truncated`].

#![expect(
    clippy::redundant_pub_crate,
    reason = "the converter is driven from the crate root and the span walk"
)]

use std::collections::HashMap;

use tsrs_ast::{self as ast, Symbol, SymbolFlags};
use tsrs_checker::{
    Checker, ElementFlags, LiteralValue, ObjectFlags, Signature, SignatureKind, Type, TypeFlags,
};
use tsrs_core::P;

use crate::model::{
    TsDeferredKind, TsIndexSignature, TsLocation, TsObjectType, TsParameter, TsProperty,
    TsSignature, TsSymbolRef, TsTupleElement, TsTupleElementKind, TsType, TsTypeId, TsTypeKind,
    TsTypeTable,
};

/// Structural nesting beyond which object types are recorded as [`TsTypeKind::Truncated`].
const MAX_STRUCTURAL_DEPTH: usize = 24;

/// Intrinsic flags and the model kind each one maps to, checked in order.
const INTRINSICS: [(TypeFlags, TsTypeKind); 12] = [
    (TypeFlags::Any, TsTypeKind::Any),
    (TypeFlags::Unknown, TsTypeKind::Unknown),
    (TypeFlags::Never, TsTypeKind::Never),
    (TypeFlags::Void, TsTypeKind::Void),
    (TypeFlags::Undefined, TsTypeKind::Undefined),
    (TypeFlags::Null, TsTypeKind::Null),
    (TypeFlags::String, TsTypeKind::String),
    (TypeFlags::Number, TsTypeKind::Number),
    (TypeFlags::Boolean, TsTypeKind::Boolean),
    (TypeFlags::BigInt, TsTypeKind::BigInt),
    (TypeFlags::ESSymbol, TsTypeKind::Symbol),
    (TypeFlags::NonPrimitive, TsTypeKind::NonPrimitive),
];

/// Deferred generic operators and the model kind each one maps to.
const DEFERRED: [(TypeFlags, TsDeferredKind); 6] = [
    (TypeFlags::Conditional, TsDeferredKind::Conditional),
    (TypeFlags::IndexedAccess, TsDeferredKind::IndexedAccess),
    (TypeFlags::Index, TsDeferredKind::Index),
    (TypeFlags::TemplateLiteral, TsDeferredKind::TemplateLiteral),
    (TypeFlags::StringMapping, TsDeferredKind::StringMapping),
    (TypeFlags::Substitution, TsDeferredKind::Substitution),
];

/// Converts checker types into a [`TsTypeTable`].
pub(crate) struct Converter<'checker> {
    /// The single program checker every type came from.
    checker: &'checker mut Checker,
    /// Output arena.
    table: TsTypeTable,
    /// tsrs type → interned id.
    seen: HashMap<P<Type>, TsTypeId>,
    /// Current structural expansion depth.
    depth: usize,
}

impl<'checker> Converter<'checker> {
    /// Creates a converter over `checker`.
    pub(crate) fn new(checker: &'checker mut Checker) -> Self {
        Self { checker, table: TsTypeTable::default(), seen: HashMap::new(), depth: 0 }
    }

    /// Gives mutable access to the checker for queries outside conversion.
    pub(crate) const fn checker(&mut self) -> &mut Checker {
        self.checker
    }

    /// Finishes conversion and returns the table.
    pub(crate) fn finish(self) -> TsTypeTable {
        self.table
    }

    /// Converts `ty`, returning its interned id.
    pub(crate) fn convert(&mut self, ty: P<Type>) -> TsTypeId {
        if let Some(&id) = self.seen.get(&ty) {
            return id;
        }
        let id = self.table.reserve();
        self.seen.insert(ty, id);
        let display = self.checker.type_to_string_exported(ty);
        let kind = self.kind_of(ty);
        self.table.fill(id, TsType { kind, display });
        id
    }

    /// Classifies `ty` and converts its constituents.
    fn kind_of(&mut self, ty: P<Type>) -> TsTypeKind {
        let flags = ty.flags();
        if let Some((_, kind)) = INTRINSICS.iter().find(|(flag, _)| flags.intersects(*flag)) {
            return kind.clone();
        }
        if flags.intersects(TypeFlags::EnumLiteral) && !flags.intersects(TypeFlags::Union) {
            return self.enum_member_kind(ty);
        }
        if let Some(literal) = literal_kind(ty) {
            return literal;
        }
        if flags.intersects(TypeFlags::UniqueESSymbol) {
            return TsTypeKind::UniqueSymbol;
        }
        if flags.intersects(TypeFlags::UnionOrIntersection) {
            return self.union_or_intersection_kind(ty);
        }
        if flags.intersects(TypeFlags::TypeParameter) {
            return self.type_parameter_kind(ty);
        }
        if flags.intersects(TypeFlags::Object) {
            return self.object_kind(ty);
        }
        DEFERRED
            .iter()
            .find(|(flag, _)| flags.intersects(*flag))
            .map_or(TsTypeKind::Truncated, |(_, kind)| TsTypeKind::Deferred(*kind))
    }

    /// An enum member: its literal value plus the declaring enum.
    ///
    /// Members of a computed (non-literal) enum carry no literal value; the
    /// checker types them as `number`, so the value slot is `number` too.
    fn enum_member_kind(&mut self, ty: P<Type>) -> TsTypeKind {
        let (value_kind, display) =
            literal_kind_and_display(ty).unwrap_or_else(|| (TsTypeKind::Number, "number".to_owned()));
        let parent = ty.symbol().and_then(|member| self.checker.get_parent_of_symbol_exported(member));
        let Some(enum_declaration) = parent else {
            return value_kind;
        };
        let enum_symbol = self.symbol_ref(enum_declaration);
        let value = self.table.reserve();
        self.table.fill(value, TsType { kind: value_kind, display });
        TsTypeKind::EnumMember { enum_symbol, value }
    }

    /// A union (possibly an enum type) or an intersection.
    fn union_or_intersection_kind(&mut self, ty: P<Type>) -> TsTypeKind {
        let members: Vec<TsTypeId> = ty.types().iter().map(|&member| self.convert(member)).collect();
        if !ty.flags().intersects(TypeFlags::Union) {
            return TsTypeKind::Intersection(members);
        }
        let enum_symbol = ty
            .symbol()
            .filter(|symbol| symbol.flags().intersects(SymbolFlags::Enum))
            .map(|symbol| self.symbol_ref(symbol));
        TsTypeKind::Union { members, enum_symbol }
    }

    /// A free type parameter with its constraint.
    fn type_parameter_kind(&mut self, ty: P<Type>) -> TsTypeKind {
        let name = ty.symbol().map_or_else(String::new, |symbol| symbol.name().to_owned());
        let constraint = self
            .checker
            .get_constraint_of_type_parameter_exported(ty)
            .map(|constraint| self.convert(constraint));
        TsTypeKind::TypeParameter { name, constraint }
    }

    /// Arrays, tuples, nominal references and structural objects.
    fn object_kind(&mut self, ty: P<Type>) -> TsTypeKind {
        if self.checker.is_array_type_exported(ty) {
            let readonly = ty
                .target()
                .and_then(|target| target.symbol())
                .is_some_and(|symbol| symbol.name() == "ReadonlyArray");
            let element = self.type_arguments(ty, 1).first().copied();
            return element.map_or(TsTypeKind::Truncated, |id| TsTypeKind::Array { element: id, readonly });
        }
        if tsrs_checker::is_tuple_type_exported(ty) {
            return self.tuple_kind(ty);
        }
        if ty.object_flags().intersects(ObjectFlags::ClassOrInterface | ObjectFlags::Reference)
            && let Some(kind) = self.named_kind(ty)
        {
            return kind;
        }
        self.structural_kind(ty)
    }

    /// A tuple reference: element types paired with their arity flags.
    fn tuple_kind(&mut self, ty: P<Type>) -> TsTypeKind {
        let Some(target) = ty.target() else {
            return TsTypeKind::Truncated;
        };
        let tuple = target.as_tuple_type();
        let flags = tuple.element_flags();
        let arguments = self.type_arguments(ty, flags.len());
        let elements = arguments
            .into_iter()
            .zip(flags)
            .map(|(element, flag)| TsTupleElement { ty: element, kind: element_kind(flag) })
            .collect();
        TsTypeKind::Tuple { elements, readonly: tuple.is_readonly() }
    }

    /// A class or interface, referenced nominally by its declaring symbol.
    fn named_kind(&mut self, ty: P<Type>) -> Option<TsTypeKind> {
        let target = ty.target().unwrap_or(ty);
        let declared = target.symbol()?;
        if !declared.flags().intersects(SymbolFlags::Class | SymbolFlags::Interface) {
            return None;
        }
        let arity = target.try_as_interface_type().map_or(0, |interface| interface.type_parameters().len());
        let type_arguments = self.type_arguments(ty, arity);
        Some(TsTypeKind::Named { symbol: self.symbol_ref(declared), type_arguments })
    }

    /// The first `arity` type arguments of a reference, dropping the implicit `this` argument.
    ///
    /// Only `ObjectFlags::Reference` types carry type arguments; like
    /// TypeScript's `getTypeArguments`, the checker asserts on anything else
    /// (a non-generic class or interface is its own declared type).
    fn type_arguments(&mut self, ty: P<Type>, arity: usize) -> Vec<TsTypeId> {
        if !ty.object_flags().intersects(ObjectFlags::Reference) {
            return Vec::new();
        }
        let arguments = self.checker.get_type_arguments_exported(ty);
        arguments.iter().take(arity).map(|&argument| self.convert(argument)).collect()
    }

    /// An anonymous, mapped or object-literal type, expanded to its members.
    fn structural_kind(&mut self, ty: P<Type>) -> TsTypeKind {
        if self.depth >= MAX_STRUCTURAL_DEPTH {
            return TsTypeKind::Truncated;
        }
        self.depth = self.depth.saturating_add(1);
        let object = TsObjectType {
            symbol: ty.symbol().map(|symbol| self.symbol_ref(symbol)),
            properties: self.properties(ty),
            call_signatures: self.signatures(ty, SignatureKind::Call),
            construct_signatures: self.signatures(ty, SignatureKind::Construct),
            index_signatures: self.index_signatures(ty),
        };
        self.depth = self.depth.saturating_sub(1);
        TsTypeKind::Object(object)
    }

    /// Resolved properties of a structural type.
    fn properties(&mut self, ty: P<Type>) -> Vec<TsProperty> {
        let symbols = self.checker.get_properties_of_type_exported(ty);
        symbols.iter().map(|&symbol| self.property(symbol)).collect()
    }

    /// One property: its type and modifiers.
    ///
    /// Without `exactOptionalPropertyTypes`, TypeScript types an optional
    /// member `a?: T` as `T | undefined`, and `a?: T | undefined` is the same
    /// type. The model records optionality once, in [`TsProperty::optional`],
    /// and strips that `undefined` from the type, so lowering sees `T` plus the
    /// flag (Rust's `Option<T>`) instead of a two-armed union.
    fn property(&mut self, symbol: P<Symbol>) -> TsProperty {
        let optional = symbol.flags().intersects(SymbolFlags::Optional);
        let mut property_ty = self.checker.get_type_of_symbol_exported(symbol);
        if optional {
            property_ty = self.checker.remove_missing_or_undefined_type_exported(property_ty);
        }
        TsProperty {
            name: symbol.name().to_owned(),
            ty: self.convert(property_ty),
            optional,
            readonly: self.checker.is_readonly_symbol_exported(symbol),
        }
    }

    /// Call or construct signatures of `ty`.
    fn signatures(&mut self, ty: P<Type>, kind: SignatureKind) -> Vec<TsSignature> {
        let signatures = self.checker.get_signatures_of_type_exported(ty, kind);
        signatures.iter().map(|&signature| self.signature(signature)).collect()
    }

    /// Converts one signature.
    pub(crate) fn signature(&mut self, signature: P<Signature>) -> TsSignature {
        let type_parameters =
            signature.type_parameters().iter().map(|&parameter| self.convert(parameter)).collect();
        let parameters = signature
            .parameters()
            .iter()
            .map(|&parameter| {
                let parameter_ty = self.checker.get_type_of_symbol_exported(parameter);
                TsParameter { name: parameter.name().to_owned(), ty: self.convert(parameter_ty) }
            })
            .collect();
        let return_ty = self.checker.get_return_type_of_signature_exported(signature);
        TsSignature {
            type_parameters,
            parameters,
            min_argument_count: signature.min_argument_count(),
            has_rest: self.checker.has_effective_rest_parameter_exported(signature),
            return_type: self.convert(return_ty),
        }
    }

    /// Index signatures of `ty`.
    fn index_signatures(&mut self, ty: P<Type>) -> Vec<TsIndexSignature> {
        let infos = self.checker.get_index_infos_of_type_exported(ty);
        infos
            .iter()
            .map(|info| TsIndexSignature {
                key: self.convert(info.key_type()),
                value: self.convert(info.value_type()),
                readonly: info.is_readonly(),
            })
            .collect()
    }

    /// Identity of a declared symbol.
    fn symbol_ref(&mut self, symbol: P<Symbol>) -> TsSymbolRef {
        TsSymbolRef {
            name: symbol.name().to_owned(),
            qualified_name: self.checker.get_fully_qualified_name_exported(symbol),
            declaration: symbol.declarations().first().and_then(|&node| location_of(node)),
        }
    }
}

/// The model kind of a string/number/boolean/bigint literal type, if `ty` is one.
fn literal_kind(ty: P<Type>) -> Option<TsTypeKind> {
    literal_kind_and_display(ty).map(|(kind, _)| kind)
}

/// A literal type's model kind together with its source spelling.
///
/// The spelling is used for literal slots Smelt creates itself (an enum
/// member's value), where the checker's `typeToString` would print the member
/// name (`Color.Green`) instead of the value.
fn literal_kind_and_display(ty: P<Type>) -> Option<(TsTypeKind, String)> {
    if !ty.flags().intersects(TypeFlags::Literal) {
        return None;
    }
    Some(match ty.as_literal_type().value()? {
        LiteralValue::String(text) => (TsTypeKind::StringLiteral(text.to_owned()), format!("{text:?}")),
        LiteralValue::Number(number) => (TsTypeKind::NumberLiteral(number.0), number.0.to_string()),
        LiteralValue::Boolean(value) => (TsTypeKind::BooleanLiteral(value), value.to_string()),
        LiteralValue::BigInt(value) => (TsTypeKind::BigIntLiteral(value.to_string()), format!("{value}n")),
    })
}

/// Maps tuple element flags to the model's arity role.
const fn element_kind(flags: ElementFlags) -> TsTupleElementKind {
    if flags.intersects(ElementFlags::Optional) {
        TsTupleElementKind::Optional
    } else if flags.intersects(ElementFlags::Rest) {
        TsTupleElementKind::Rest
    } else if flags.intersects(ElementFlags::Variadic) {
        TsTupleElementKind::Variadic
    } else {
        TsTupleElementKind::Required
    }
}

/// Source location of a declaration node, skipping leading trivia like oxc spans do.
pub(crate) fn location_of(node: P<ast::Node>) -> Option<TsLocation> {
    let file = ast::get_source_file_of_node(node)?;
    let pos = node.pos().source_offset()?;
    let end = node.end().source_offset()?;
    Some(TsLocation {
        file: file.file_name().to_owned(),
        start: tsrs_scanner::skip_trivia(file.text(), pos),
        end,
    })
}
