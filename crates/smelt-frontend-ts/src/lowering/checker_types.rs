//! Annotation types the TypeScript 7 checker evaluates for Smelt.
//!
//! Smelt lowers TypeScript annotations itself (`ty/annotations.rs`), and
//! whatever that lowering cannot evaluate erases to `Unknown`. Examples are a
//! conditional type whose arms differ, `ReturnType<H>` of a union of handler
//! types, `Router['match']`, or a mapped type. The TypeScript 7 checker
//! evaluates exactly those operators. When the transpiler embeds it,
//! [`crate::FrontendOptions::ts_facts`] carries `getTypeFromTypeNode` for
//! every annotation node, keyed by source span.
//!
//! # The rule
//!
//! When an annotation's own lowering yields `Unknown`, ask the checker what
//! the annotation evaluates to, **re-spell that type as TypeScript type syntax
//! that resolves in this module**, and lower the re-spelled annotation through
//! the ordinary [`ModuleBuilder::ts_type_to_hir`] path. Re-spelling instead of
//! mapping the checker's type straight to HIR keeps one source of truth for
//! every lowering convention: a checker-evaluated `Response | undefined`
//! lowers exactly as if the source had written `Response | undefined`
//! (nullish flattening, callable-union arity, host classes, literal unions).
//! Types are spelled structurally where they can be, and by alias name only
//! where they cannot (alias-backed object shapes).
//! The behaviour requirement is unchanged from writing the evaluated type by
//! hand. The type-preservation requirement is that the value keeps the type
//! TypeScript proved instead of `Unknown`.
//!
//! The re-spelling is conservative and declines (keeping the erasure) unless
//! every part can be named faithfully:
//!
//! - a type the checker could not evaluate (still generic, depth-truncated) or
//!   that is itself dynamic at the top (`any`, `unknown`, `object`, `symbol`);
//! - a named class, interface, enum or alias that is not visible here under
//!   its own name: lib types must not be shadowed by a module type or import,
//!   and user types must be declared in this file or imported from the file
//!   that declares them;
//! - a free type parameter that is not in scope at the annotation, and the
//!   polymorphic `this` (Smelt's own rule maps it to the enclosing class);
//! - a structural object type other than a single call signature (Smelt names
//!   anonymous object shapes after their source site, which a re-spelling
//!   cannot reproduce), except a plain-data record for an unannotated literal
//!   binding, spelled as its type literal (structural records are named by
//!   structure, so it lowers to the struct the same literal written in source
//!   lowers to).

use std::fmt::Write as _;

use oxc::allocator::Allocator;
use oxc::ast::ast::Statement;
use oxc::parser::Parser;
use oxc::span::SourceType;
use smelt_hir::{ErasureReason, Item, Type, TypeId};
use smelt_ts_model::model::{TsObjectType, TsSymbolRef, TsTupleElement, TsTupleElementKind};
use smelt_ts_model::{TsTypeFacts, TsTypeId, TsTypeKind};

use super::ModuleBuilder;
use super::literal_record_uses::LiteralDecision;

/// The outermost kind of a value, as far as overload selection compares kinds
/// (see [`ModuleBuilder::checker_call_value_kind`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::lowering) enum CheckerValueKind {
    /// A callable value (a function type, or an object with only call signatures).
    Function,
    /// An array or tuple.
    List,
    /// A string.
    String,
    /// A number.
    Number,
    /// A boolean.
    Boolean,
}

/// Nesting depth beyond which a re-spelling gives up.
const MAX_DEPTH: usize = 12;

/// TypeScript's apparent-type root interfaces; see [`Respeller::named`].
const APPARENT_TYPE_ROOTS: [&str; 4] =
    ["Object", "Function", "CallableFunction", "NewableFunction"];

/// Virtual root the checker bridge places program files under (`smelt-ts-types`).
const PROGRAM_ROOT: &str = "/smelt/";

impl ModuleBuilder<'_> {
    /// The checker-evaluated type of the annotation at `start..end`, lowered
    /// through Smelt's annotation path; `None` when the checker has no
    /// faithful answer (see the module docs).
    pub(in crate::lowering) fn checker_annotation_type(
        &mut self,
        start: u32,
        end: u32,
    ) -> Option<TypeId> {
        let facts = self.ts_facts?;
        let id = facts.type_node_id_at(&self.path, start, end)?;
        self.respell_and_lower(facts, id, start, end)
    }

    /// The checker's type for the value binding at `start..end` (an
    /// unannotated, contextually typed parameter), lowered.
    ///
    /// A class or interface declared in another crate module resolves by
    /// DECLARATION, not by spelling: a test file's `(c) => c.text('x')`
    /// handler never imports `Context`, yet TypeScript knows `c` is one. The
    /// type takes the class's declared default type arguments, which is what
    /// a bare `(c: Context) => ..` annotation lowers to. Everything else goes
    /// through the same in-scope re-spelling as annotations.
    pub(in crate::lowering) fn checker_binding_type(
        &mut self,
        start: u32,
        end: u32,
    ) -> Option<TypeId> {
        let facts = self.ts_facts?;
        let id = facts.type_id_at(&self.path, start, end)?;
        if let Some(TsTypeKind::Named { symbol, .. }) = facts.types.get(id).map(|ty| &ty.kind)
            && let Some(declared) = self.checker_declared_class_type(symbol)
        {
            return Some(declared);
        }
        self.respell_and_lower(facts, id, start, end)
    }

    /// The type TypeScript evolved for an unannotated `= []` variable whose
    /// declaration name spans `start..end`, lowered as if the source had
    /// written it as the annotation.
    ///
    /// `const removed = []; .. removed.push(v); return removed;` is `T[]` in
    /// TypeScript. A hand port declares `let mut removed: Vec<T>`. Without
    /// the checker the empty literal has no element evidence and erases. See
    /// `TsTypeFacts::evolved_array_type_id_at` for which type is taken. The
    /// re-spelling declines like an annotation does, keeping the erasure.
    pub(in crate::lowering) fn checker_evolved_array_type(
        &mut self,
        start: u32,
        end: u32,
    ) -> Option<TypeId> {
        let facts = self.ts_facts?;
        let id = facts.evolved_array_type_id_at(&self.path, start, end)?;
        self.respell_and_lower(facts, id, start, end)
    }

    /// The type the checker resolved the call at `start..end` to, lowered.
    ///
    /// The checker saw every candidate position of the call, including ones the
    /// lowered arguments no longer show. Its `unknown` is the source-level
    /// dynamic boundary and is answered as such. `any` makes no such claim (an
    /// unresolved import answers `any` too), so it is `None`, like a type that
    /// cannot be spelled here.
    pub(in crate::lowering) fn checker_call_type(
        &mut self,
        start: u32,
        end: u32,
    ) -> Option<TypeId> {
        let facts = self.ts_facts?;
        let id = facts.type_id_at(&self.path, start, end)?;
        if matches!(
            facts.types.get(id).map(|ty| &ty.kind),
            Some(TsTypeKind::Unknown)
        ) {
            return Some(self.erase(ErasureReason::SourceUnknown));
        }
        self.respell_and_lower(facts, id, start, end)
    }

    /// The return type of the function the checker types the expression at
    /// `start..end` as (an unannotated arrow or function expression), lowered.
    ///
    /// A closure with no return annotation and no contextual return is lowered
    /// with the "no hint" sentinel, so its body has to infer the return, and an
    /// `async` body returning a local Smelt could not follow erased it
    /// (`async (acc: Record<string, number>, ..) => { ..; return acc; }`). The
    /// checker inferred that return; it is the closure's return type exactly
    /// as an annotation spelling it would be. Only a single, non-generic call
    /// signature whose return re-spells here is taken (a generic closure's
    /// return names its own parameters); the checker's `unknown`/`any` return,
    /// a return mentioning `never`, and one joining `null` with `undefined`
    /// (see [`checker_type_joins_null_and_undefined`]) are no answer.
    pub(in crate::lowering) fn checker_function_return_type(
        &mut self,
        start: u32,
        end: u32,
    ) -> Option<TypeId> {
        let facts = self.ts_facts?;
        let id = facts.type_id_at(&self.path, start, end)?;
        let Some(TsTypeKind::Object(object)) = facts.types.get(id).map(|ty| &ty.kind) else {
            return None;
        };
        let [signature] = object.call_signatures.as_slice() else {
            return None;
        };
        if !signature.type_parameters.is_empty() {
            return None;
        }
        let return_type = signature.return_type;
        if checker_type_joins_null_and_undefined(facts, return_type, 0) {
            return None;
        }
        let records_before = self.ctx.krate.erasures.len();
        let lowered = self
            .respell_and_lower(facts, return_type, start, end)
            .filter(|ty| !self.type_mentions_never(*ty));
        if lowered.is_none() {
            self.ctx.krate.erasures.truncate(records_before);
        }
        lowered
    }

    /// Whether `ty` mentions a union anywhere (an optional is not one).
    fn type_mentions_union(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Union(_)) => true,
            Some(other) => other
                .children()
                .any(|child| self.type_mentions_union(child)),
            None => false,
        }
    }

    /// Whether `ty` mentions `never` anywhere.
    ///
    /// A checker return such as `never[][]` (`flatMapDeep([], n => [[n]])`,
    /// where `n` is the element of an empty array) describes a branch no value
    /// reaches; as a closure's return it would ask the body to build a `never`.
    fn type_mentions_never(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Never) => true,
            Some(other) => other
                .children()
                .any(|child| self.type_mentions_never(child)),
            None => false,
        }
    }

    /// The type TypeScript inferred for an unannotated variable whose
    /// initializer is the object or array literal `init`, declared by the
    /// binding at `start..end`, lowered as if the source had written it as
    /// the annotation.
    ///
    /// Without an annotation the literal has no contextual type: a `Dict`
    /// needs one value type for every key, so `{ id: 1, enabled: false }`
    /// became `Dict<String, SmeltUnknown>` and every property read lost its
    /// type, and a list of such records lost its elements'. The checker types
    /// the binding `{ id: number; enabled: boolean }[]`; a hand port declares
    /// exactly that, and the re-spelled annotation lowers to the struct the
    /// same written annotation gives (`decls::anonymous_record`, named by
    /// structure). Only the binding's own declaration takes it: a literal in
    /// any other position flows into a slot whose type is the authority, and
    /// the checker's view of the bare expression may be narrower than it.
    ///
    /// Only a fully representable answer is taken: its lowering records no
    /// erasure (an `any` element, an open layout), and it holds no union
    /// (`[true, null, 1, 'a']` is a list of a union codegen would erase, where
    /// the mixed literal already erases once) and no `never`.
    ///
    /// And only when the binding's uses allow it: a struct is copied wherever
    /// it is converted, so a literal whose identity, key set or key order is
    /// observed, or that is passed, returned or stored, keeps the map lowering
    /// (`literal_record_uses`, which also leaves alone every literal that
    /// holds no object literal, a spread, or a value produced elsewhere).
    pub(in crate::lowering) fn checker_literal_binding_type(
        &mut self,
        start: u32,
        end: u32,
    ) -> Option<TypeId> {
        // The struct is taken only when every use of the binding keeps the
        // value typed (`literal_record_uses`): a literal whose identity, key
        // set or key order is observed, or that is passed, returned or stored
        // somewhere a struct would be copied into, stays the map it was.
        let decision = self.literal_record_uses.decision(start)?;
        let trace = std::env::var_os("SMELT_LITERAL_RECORD_TRACE").is_some();
        if let LiteralDecision::Map(disqualifier) = decision {
            #[expect(
                clippy::print_stderr,
                reason = "SMELT_LITERAL_RECORD_TRACE is an opt-in developer trace"
            )]
            if trace {
                // Measurement only: whether the checker's type would have been
                // taken, so each disqualifier's cost can be counted.
                // The trial may declare a record struct nothing uses, so a
                // traced build is for counting, not for shipping.
                let taken = self.checker_record_type(start, end).is_some();
                eprintln!(
                    "literal-record {}:{start} map {} checker={}",
                    self.path,
                    disqualifier.name(),
                    if taken { "taken" } else { "declined" },
                );
            }
            return None;
        }
        let lowered = self.checker_record_type(start, end);
        #[expect(
            clippy::print_stderr,
            reason = "SMELT_LITERAL_RECORD_TRACE is an opt-in developer trace"
        )]
        if trace {
            eprintln!(
                "literal-record {}:{start} struct checker={}",
                self.path,
                if lowered.is_some() {
                    "taken"
                } else {
                    "declined"
                },
            );
        }
        lowered
    }

    /// The checker's type for the binding at `start..end`, lowered with
    /// plain-data object types as records, when it is fully representable
    /// (see [`Self::checker_literal_binding_type`]).
    fn checker_record_type(&mut self, start: u32, end: u32) -> Option<TypeId> {
        let facts = self.ts_facts?;
        let id = facts.type_id_at(&self.path, start, end)?;
        if checker_type_joins_null_and_undefined(facts, id, 0)
            || checker_type_has_present_undefined_property(facts, id, 0)
        {
            return None;
        }
        let records_before = self.ctx.krate.erasures.len();
        let lowered = self
            .respell_and_lower_spelling(facts, id, start, end, true)
            .filter(|ty| {
                self.ctx.krate.erasures.len() == records_before
                    && !self.type_mentions_never(*ty)
                    && !self.type_mentions_union(*ty)
            });
        if lowered.is_none() {
            self.ctx.krate.erasures.truncate(records_before);
        }
        lowered
    }

    /// The outermost kind of the type the checker resolved the call at
    /// `start..end` to, read straight off the checker's type: no re-spelling,
    /// so it answers for types Smelt cannot spell in this scope too. `None`
    /// when the checker has no answer or the kind is none of
    /// [`CheckerValueKind`]'s.
    pub(in crate::lowering) fn checker_call_value_kind(
        &self,
        start: u32,
        end: u32,
    ) -> Option<CheckerValueKind> {
        let facts = self.ts_facts?;
        let id = facts.type_id_at(&self.path, start, end)?;
        match &facts.types.get(id)?.kind {
            TsTypeKind::Object(object)
                if !object.call_signatures.is_empty() && object.properties.is_empty() =>
            {
                Some(CheckerValueKind::Function)
            }
            TsTypeKind::Array { .. } | TsTypeKind::Tuple { .. } => Some(CheckerValueKind::List),
            TsTypeKind::String | TsTypeKind::StringLiteral(_) => Some(CheckerValueKind::String),
            TsTypeKind::Number | TsTypeKind::NumberLiteral(_) => Some(CheckerValueKind::Number),
            TsTypeKind::Boolean | TsTypeKind::BooleanLiteral(_) => Some(CheckerValueKind::Boolean),
            _ => None,
        }
    }

    /// `resolved` where it widens `inferred`, else `inferred`.
    ///
    /// `inferred` must not promise more than the checker does. Only a widening
    /// is taken: `inferred` is what the emitted definition answers at this site,
    /// and a claim narrower than it would describe a value the definition never
    /// returns. `fork([], f)` is `never[]` to the checker, and `reduceAsync`
    /// with an unbound `U` hands back `undefined` where an overload promises
    /// `number`.
    pub(in crate::lowering) fn widened_call_type(
        &self,
        resolved: Option<TypeId>,
        inferred: TypeId,
    ) -> TypeId {
        resolved
            .filter(|resolved| self.type_accepts_all_of(*resolved, inferred))
            .unwrap_or(inferred)
    }

    /// Whether every value of `narrow` is a value of `wide`, position by position.
    pub(in crate::lowering) fn type_accepts_all_of(&self, wide: TypeId, narrow: TypeId) -> bool {
        if wide == narrow {
            return true;
        }
        let (Some(wide_ty), Some(narrow_ty)) = (
            self.ctx.krate.types.get(wide),
            self.ctx.krate.types.get(narrow),
        ) else {
            return false;
        };
        match (wide_ty, narrow_ty) {
            (_, Type::Unknown) => false,
            (Type::Unknown, _) => true,
            (Type::Optional(_), Type::None) => true,
            (Type::Optional(wide_item), Type::Optional(narrow_item)) => {
                self.type_accepts_all_of(*wide_item, *narrow_item)
            }
            (Type::Optional(wide_item), _) => self.type_accepts_all_of(*wide_item, narrow),
            (_, Type::Union(members)) => members
                .iter()
                .all(|member| self.type_accepts_all_of(wide, *member)),
            (Type::Union(members), _) => members
                .iter()
                .any(|member| self.type_accepts_all_of(*member, narrow)),
            (Type::List(wide_item), Type::List(narrow_item))
            | (Type::Set(wide_item), Type::Set(narrow_item))
            | (Type::Future(wide_item), Type::Future(narrow_item)) => {
                self.type_accepts_all_of(*wide_item, *narrow_item)
            }
            (Type::Dict(wide_key, wide_value), Type::Dict(narrow_key, narrow_value))
            | (Type::JsMap(wide_key, wide_value), Type::JsMap(narrow_key, narrow_value)) => {
                self.type_accepts_all_of(*wide_key, *narrow_key)
                    && self.type_accepts_all_of(*wide_value, *narrow_value)
            }
            (Type::Tuple(wide_items), Type::Tuple(narrow_items)) => {
                wide_items.len() == narrow_items.len()
                    && wide_items
                        .iter()
                        .zip(narrow_items)
                        .all(|(wide, narrow)| self.type_accepts_all_of(*wide, *narrow))
            }
            (
                Type::Class {
                    name: wide_name,
                    args: wide_args,
                },
                Type::Class {
                    name: narrow_name,
                    args: narrow_args,
                },
            ) => {
                wide_name == narrow_name
                    && wide_args.len() == narrow_args.len()
                    && wide_args
                        .iter()
                        .zip(narrow_args)
                        .all(|(wide, narrow)| self.type_accepts_all_of(*wide, *narrow))
            }
            _ => false,
        }
    }

    /// Re-spells checker type `id` in this scope and lowers it; `None` for a
    /// type that is dynamic or unevaluated at the top or cannot be spelled.
    fn respell_and_lower(
        &mut self,
        facts: &TsTypeFacts,
        id: TsTypeId,
        start: u32,
        end: u32,
    ) -> Option<TypeId> {
        self.respell_and_lower_spelling(facts, id, start, end, false)
    }

    /// [`Self::respell_and_lower`], spelling plain-data object types as type
    /// literals when `records` is set (see [`Respeller::plain_record`]).
    fn respell_and_lower_spelling(
        &mut self,
        facts: &TsTypeFacts,
        id: TsTypeId,
        start: u32,
        end: u32,
        records: bool,
    ) -> Option<TypeId> {
        let top = facts.types.get(id)?;
        if matches!(
            top.kind,
            TsTypeKind::Any
                | TsTypeKind::Unknown
                | TsTypeKind::NonPrimitive
                | TsTypeKind::Symbol
                | TsTypeKind::UniqueSymbol
                | TsTypeKind::Deferred(_)
                | TsTypeKind::Truncated
        ) {
            return None;
        }
        let text = Respeller {
            builder: self,
            facts,
            annotation: (start, end),
            records,
        }
        .respell(id, 0)?;
        self.lower_respelled_annotation(&text, start, end)
    }

    /// The HIR type of the class or interface `symbol` declares in another
    /// crate module, at its default type arguments.
    ///
    /// Declines for a lib type (those resolve by name), a declaration whose
    /// module or item is not lowered yet, and a type parameter without a
    /// default (no argument would be faithful without mapping the checker's).
    fn checker_declared_class_type(&mut self, symbol: &TsSymbolRef) -> Option<TypeId> {
        let declaration = symbol.declaration.as_ref()?;
        if is_lib_file(&declaration.file) {
            return None;
        }
        let target = program_relative(&declaration.file);
        let module = self
            .ctx
            .krate
            .modules
            .iter()
            .find(|module| module_program_relative(&module.source.path) == target)?;
        let (name, type_params) = module.items.iter().find_map(|item| {
            let found = self.ctx.krate.items.get(usize::try_from(item.0).ok()?)?;
            let (name, type_params) = match found {
                Item::Class(class) => (class.name, &class.type_params),
                Item::Interface(interface) => (interface.name, &interface.type_params),
                Item::Function(_)
                | Item::TypeAlias(_)
                | Item::Const(_)
                | Item::MutableGlobal(_) => return None,
            };
            let original = self
                .ctx
                .krate
                .names
                .get(name)
                .or_else(|| self.ctx.krate.symbols.source(name));
            (original == Some(symbol.name.as_str())).then(|| (name, type_params.clone()))
        })?;
        let args = type_params
            .iter()
            .map(|param| param.default)
            .collect::<Option<Vec<_>>>()?;
        Some(self.ctx.krate.types.intern(Type::Class { name, args }))
    }

    /// Parses `text` as a type annotation and lowers it in this module's scope.
    ///
    /// Checker facts are switched off for the nested lowering (its synthetic
    /// spans would address unrelated nodes of this file), and erasure records
    /// it makes are re-anchored to the original annotation's span.
    fn lower_respelled_annotation(&mut self, text: &str, start: u32, end: u32) -> Option<TypeId> {
        let allocator = Allocator::default();
        let source = format!("type __SmeltCheckerType = {text};");
        let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
        if !parsed.diagnostics.is_empty() {
            return None;
        }
        let Some(Statement::TSTypeAliasDeclaration(alias)) = parsed.program.body.first() else {
            return None;
        };
        let saved_facts = self.ts_facts.take();
        let records_before = self.ctx.krate.erasures.len();
        let lowered = self.ts_type_to_hir(&alias.type_annotation).ok();
        self.ts_facts = saved_facts;
        let anchor = self.span(start, end);
        for record in self.ctx.krate.erasures.iter_mut().skip(records_before) {
            record.span = anchor;
        }
        lowered.filter(|ty| !matches!(self.ctx.krate.types.get(*ty), Some(Type::Unknown)))
    }

    /// Whether `symbol` can be spelled by its own name in this module and
    /// mean the declaration the checker resolved.
    fn type_name_visible(&self, symbol: &TsSymbolRef) -> bool {
        let name = symbol.name.as_str();
        if !is_identifier(name) {
            return false;
        }
        let Some(declaration) = &symbol.declaration else {
            return false;
        };
        if is_lib_file(&declaration.file) {
            return !self.types.module_declares_type_name(name)
                && self.imports.import_source(name).is_none();
        }
        if same_file(&declaration.file, &self.path) {
            return self.types.module_declares_type_name(name) || self.source_declares_type(name);
        }
        let Some(specifier) = self.imports.import_source(name) else {
            return false;
        };
        self.imports
            .imported_name(name)
            .is_none_or(|imported| imported == name)
            && specifier_resolves_to(&self.path, specifier, &declaration.file)
    }
}

/// Renders checker types as TypeScript type syntax resolvable in one module.
struct Respeller<'builder, 'ctx> {
    /// The module the annotation lives in (scope for names and type parameters).
    builder: &'builder ModuleBuilder<'ctx>,
    /// The checker facts holding the types.
    facts: &'builder TsTypeFacts,
    /// Byte span of the annotation being re-spelled, in the builder's file.
    annotation: (u32, u32),
    /// Whether plain-data object types are spelled as type literals. Only an
    /// unannotated literal binding asks for it
    /// ([`ModuleBuilder::checker_literal_binding_type`]): there the object
    /// type IS the literal's shape, while an object type the checker computes
    /// for an annotation (`ReturnType<..>` of a branded function) is the
    /// result of type-level operations Smelt's other lowering owns.
    records: bool,
}

impl Respeller<'_, '_> {
    /// Spells type `id`, or `None` when it cannot be spelled faithfully here.
    fn respell(&self, id: TsTypeId, depth: usize) -> Option<String> {
        if depth > MAX_DEPTH {
            return None;
        }
        let ty = self.facts.types.get(id)?;
        let next = depth.saturating_add(1);
        // Structure first: an alias name re-enters the very lowering that
        // erased (Smelt resolves some aliases only in their declaring context,
        // and a bare reference then falls back to a nominal class). The alias
        // is spelled only when the structure cannot be, such as an alias-backed
        // object shape.
        keyword(&ty.kind)
            .map(str::to_owned)
            .or_else(|| self.structure(&ty.kind, next))
            .or_else(|| {
                let alias = ty.alias.as_ref()?;
                if !self.builder.type_name_visible(&alias.symbol) || self.declares(&alias.symbol) {
                    return None;
                }
                self.named(&alias.symbol.name, &alias.type_arguments, next)
            })
    }

    /// Whether `symbol`'s declaration encloses the annotation being re-spelled.
    ///
    /// An alias's own body evaluates to a type carrying that alias, so naming
    /// the alias there would define it as itself: the re-spelled reference
    /// resolves to whatever the alias was predeclared as, bypassing the body.
    fn declares(&self, symbol: &TsSymbolRef) -> bool {
        let (start, end) = self.annotation;
        symbol.declaration.as_ref().is_some_and(|declaration| {
            same_file(&declaration.file, &self.builder.path)
                && declaration.start <= start
                && end <= declaration.end
        })
    }

    /// Spells literal and composite kinds.
    fn structure(&self, kind: &TsTypeKind, depth: usize) -> Option<String> {
        match kind {
            TsTypeKind::StringLiteral(text) => Some(string_literal(text)),
            TsTypeKind::NumberLiteral(value) => number_literal(*value),
            TsTypeKind::BooleanLiteral(value) => Some(value.to_string()),
            TsTypeKind::BigIntLiteral(value) => Some(format!("{value}n")),
            TsTypeKind::EnumMember { enum_symbol, .. }
            | TsTypeKind::Union {
                enum_symbol: Some(enum_symbol),
                ..
            } => self
                .builder
                .type_name_visible(enum_symbol)
                .then(|| enum_symbol.name.clone()),
            TsTypeKind::Union {
                members,
                enum_symbol: None,
            } => self.joined(members, " | ", depth),
            TsTypeKind::Intersection(members) => self.joined(members, " & ", depth),
            TsTypeKind::TypeParameter { name, is_this, .. } => self.type_parameter(name, *is_this),
            TsTypeKind::Array { element, readonly } => {
                let element = self.respell(*element, depth)?;
                Some(format!(
                    "{}({element})[]",
                    if *readonly { "readonly " } else { "" }
                ))
            }
            TsTypeKind::Tuple { elements, readonly } => self.tuple(elements, *readonly, depth),
            TsTypeKind::Named {
                symbol,
                type_arguments,
            } => {
                if is_node_timer_handle(symbol) {
                    return Some("number".to_owned());
                }
                if self.builder.type_name_visible(symbol) {
                    self.named(&symbol.name, type_arguments, depth)
                } else {
                    None
                }
            }
            TsTypeKind::Object(object) => self.callable(object, depth).or_else(|| {
                self.records
                    .then(|| self.plain_record(object, depth))
                    .flatten()
            }),
            TsTypeKind::Any
            | TsTypeKind::Unknown
            | TsTypeKind::Never
            | TsTypeKind::Void
            | TsTypeKind::Undefined
            | TsTypeKind::Null
            | TsTypeKind::String
            | TsTypeKind::Number
            | TsTypeKind::Boolean
            | TsTypeKind::BigInt
            | TsTypeKind::Symbol
            | TsTypeKind::NonPrimitive
            | TsTypeKind::UniqueSymbol
            | TsTypeKind::Deferred(_)
            | TsTypeKind::Truncated => None,
        }
    }

    /// Members joined by `separator`, each parenthesised so precedence survives.
    fn joined(&self, members: &[TsTypeId], separator: &str, depth: usize) -> Option<String> {
        let parts = members
            .iter()
            .map(|&member| self.respell(member, depth).map(|text| format!("({text})")))
            .collect::<Option<Vec<_>>>()?;
        Some(parts.join(separator))
    }

    /// `Name` or `Name<A, B>`.
    ///
    /// TypeScript's apparent-type roots are declined: `Object`, `Function`,
    /// `CallableFunction` and `NewableFunction` are the lib interfaces every
    /// object or function is assignable to, so they promise no layout at all
    /// (the open-layout boundary Smelt erases), and naming one would turn
    /// "any object" into a nominal class nothing constructs.
    fn named(&self, name: &str, arguments: &[TsTypeId], depth: usize) -> Option<String> {
        if APPARENT_TYPE_ROOTS.contains(&name) {
            return None;
        }
        if arguments.is_empty() {
            return Some(name.to_owned());
        }
        let parts = arguments
            .iter()
            .map(|&argument| self.respell(argument, depth))
            .collect::<Option<Vec<_>>>()?;
        Some(format!("{name}<{}>", parts.join(", ")))
    }

    /// A type parameter in scope.
    ///
    /// The polymorphic `this` is declined: Smelt's own rule lowers it to the
    /// enclosing class (`TSThisType` in `ty/annotations.rs`), and where no
    /// class encloses the annotation the checker's class name is not
    /// guaranteed to resolve to the same declaration in this scope.
    fn type_parameter(&self, name: &str, is_this: bool) -> Option<String> {
        if is_this {
            return None;
        }
        self.builder.types.param_type(name).map(|_| name.to_owned())
    }

    /// `[A, B?, ...C[]]`, optionally `readonly`.
    fn tuple(&self, elements: &[TsTupleElement], readonly: bool, depth: usize) -> Option<String> {
        let parts = elements
            .iter()
            .map(|element| {
                let text = self.respell(element.ty, depth)?;
                Some(match element.kind {
                    TsTupleElementKind::Required => text,
                    TsTupleElementKind::Optional => format!("({text})?"),
                    TsTupleElementKind::Rest => format!("...({text})[]"),
                    TsTupleElementKind::Variadic => format!("...{text}"),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(format!(
            "{}[{}]",
            if readonly { "readonly " } else { "" },
            parts.join(", ")
        ))
    }

    /// A plain-data structural type (named properties only), as a type literal.
    ///
    /// `{ id: number; enabled: boolean }` is spelled as written, so it lowers
    /// exactly as the same type literal written in source does: a shared
    /// value type keeps the `Dict` lowering, differing ones the synthesized
    /// struct `decls::anonymous_record` names by structure, the same struct
    /// for the same shape wherever it is spelled. A type with call, construct
    /// or index signatures, with no property, or with a property name that
    /// cannot be written as a key is declined.
    fn plain_record(&self, object: &TsObjectType, depth: usize) -> Option<String> {
        if object.properties.is_empty()
            || !object.call_signatures.is_empty()
            || !object.construct_signatures.is_empty()
            || !object.index_signatures.is_empty()
        {
            return None;
        }
        let members = object
            .properties
            .iter()
            .map(|property| {
                let key = if is_identifier(&property.name) {
                    property.name.clone()
                } else if property.name.starts_with("__@") {
                    // A symbol-keyed property: no source key spells it.
                    return None;
                } else {
                    string_literal(&property.name)
                };
                let ty = self.respell(property.ty, depth)?;
                let optional = if property.optional { "?" } else { "" };
                Some(format!("{key}{optional}: {ty};"))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(format!("{{ {} }}", members.join(" ")))
    }

    /// A structural type that is exactly one call signature, as a function type.
    fn callable(&self, object: &TsObjectType, depth: usize) -> Option<String> {
        let [signature] = object.call_signatures.as_slice() else {
            return None;
        };
        if !object.properties.is_empty()
            || !object.construct_signatures.is_empty()
            || !object.index_signatures.is_empty()
            || !signature.type_parameters.is_empty()
        {
            return None;
        }
        let last = signature.parameters.len().checked_sub(1);
        let parameters = signature
            .parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let text = self.respell(parameter.ty, depth)?;
                Some(if signature.has_rest && Some(index) == last {
                    format!("...p{index}: {text}")
                } else if index >= signature.min_argument_count {
                    format!("p{index}?: {text}")
                } else {
                    format!("p{index}: {text}")
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let result = self.respell(signature.return_type, depth)?;
        Some(format!("(({}) => {result})", parameters.join(", ")))
    }
}

/// The checker types a value or slot holds, one level down: union members,
/// array and tuple elements, object properties, type arguments.
fn checker_type_children(facts: &TsTypeFacts, id: TsTypeId) -> Vec<TsTypeId> {
    match facts.types.get(id).map(|ty| &ty.kind) {
        Some(TsTypeKind::Union { members, .. } | TsTypeKind::Intersection(members)) => {
            members.clone()
        }
        Some(TsTypeKind::Array { element, .. }) => vec![*element],
        Some(TsTypeKind::Tuple { elements, .. }) => {
            elements.iter().map(|element| element.ty).collect()
        }
        Some(TsTypeKind::Object(object)) => object
            .properties
            .iter()
            .map(|property| property.ty)
            .collect(),
        Some(TsTypeKind::Named { type_arguments, .. }) => type_arguments.clone(),
        _ => Vec::new(),
    }
}

/// Whether checker type `id` holds a union of both `null` and `undefined`.
///
/// Smelt carries `T | null | undefined` as one `Option<T>`, which is the
/// chosen ABI for a slot, but a value the source distinguishes by which
/// nullish it is (`[1, null, undefined].filter(isNull)`, a customizer that
/// returns `null` to mean something else than returning nothing) would lose
/// the difference if a checker answer were adopted for it. Such answers are
/// declined and the value keeps its own lowering.
fn checker_type_joins_null_and_undefined(facts: &TsTypeFacts, id: TsTypeId, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    if let Some(TsTypeKind::Union { members, .. }) = facts.types.get(id).map(|ty| &ty.kind) {
        let has = |wanted: fn(&TsTypeKind) -> bool| {
            members
                .iter()
                .any(|member| facts.types.get(*member).is_some_and(|ty| wanted(&ty.kind)))
        };
        if has(|kind| matches!(kind, TsTypeKind::Null))
            && has(|kind| matches!(kind, TsTypeKind::Undefined | TsTypeKind::Void))
        {
            return true;
        }
    }
    checker_type_children(facts, id)
        .into_iter()
        .any(|child| checker_type_joins_null_and_undefined(facts, child, depth + 1))
}

/// Whether checker type `id` holds an object with a REQUIRED property whose
/// type admits `undefined` (`{ b: undefined, c: 3 }`).
///
/// Such a property is present even when its value is `undefined` (`'b' in
/// source` holds), while a struct field holding `Option<T>` erases an empty
/// value by omitting the key, so a literal of that shape is declined. An
/// optional property (`b?: T`) is absent-or-valued and representable.
fn checker_type_has_present_undefined_property(
    facts: &TsTypeFacts,
    id: TsTypeId,
    depth: usize,
) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    if let Some(TsTypeKind::Object(object)) = facts.types.get(id).map(|ty| &ty.kind)
        && object
            .properties
            .iter()
            .any(|property| !property.optional && checker_type_admits_undefined(facts, property.ty))
    {
        return true;
    }
    checker_type_children(facts, id)
        .into_iter()
        .any(|child| checker_type_has_present_undefined_property(facts, child, depth + 1))
}

/// Whether checker type `id` is `undefined`/`void` or a union with such a member.
fn checker_type_admits_undefined(facts: &TsTypeFacts, id: TsTypeId) -> bool {
    match facts.types.get(id).map(|ty| &ty.kind) {
        Some(TsTypeKind::Undefined | TsTypeKind::Void) => true,
        Some(TsTypeKind::Union { members, .. }) => members.iter().any(|member| {
            matches!(
                facts.types.get(*member).map(|ty| &ty.kind),
                Some(TsTypeKind::Undefined | TsTypeKind::Void)
            )
        }),
        _ => false,
    }
}

/// The keyword spelling of an intrinsic kind.
const fn keyword(kind: &TsTypeKind) -> Option<&'static str> {
    Some(match kind {
        TsTypeKind::Any => "any",
        TsTypeKind::Unknown => "unknown",
        TsTypeKind::Never => "never",
        TsTypeKind::Void => "void",
        TsTypeKind::Undefined => "undefined",
        TsTypeKind::Null => "null",
        TsTypeKind::String => "string",
        TsTypeKind::Number => "number",
        TsTypeKind::Boolean => "boolean",
        TsTypeKind::BigInt => "bigint",
        TsTypeKind::Symbol | TsTypeKind::UniqueSymbol => "symbol",
        TsTypeKind::NonPrimitive => "object",
        TsTypeKind::StringLiteral(_)
        | TsTypeKind::NumberLiteral(_)
        | TsTypeKind::BooleanLiteral(_)
        | TsTypeKind::BigIntLiteral(_)
        | TsTypeKind::EnumMember { .. }
        | TsTypeKind::Union { .. }
        | TsTypeKind::Intersection(_)
        | TsTypeKind::TypeParameter { .. }
        | TsTypeKind::Array { .. }
        | TsTypeKind::Tuple { .. }
        | TsTypeKind::Named { .. }
        | TsTypeKind::Object(_)
        | TsTypeKind::Deferred(_)
        | TsTypeKind::Truncated => return None,
    })
}

/// Whether `name` is a plain identifier (checker-internal names like `__type` are not spelled).
fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|rest| rest.is_alphanumeric() || rest == '_' || rest == '$')
        && !name.starts_with("__")
}

/// Node's timer handle interfaces, by their qualified names in `@types/node`
/// (declared inside `declare global { namespace NodeJS { .. } }`).
const NODE_TIMER_HANDLES: [&str; 3] = [
    "global.NodeJS.Timeout",
    "global.NodeJS.Timer",
    "global.NodeJS.Immediate",
];

/// Whether `symbol` is one of Node's timer handles (`NodeJS.Timeout`, the
/// type `setTimeout` returns under `@types/node`), declared in the vendored
/// `@types/node`.
///
/// Smelt's runtime hands out a timer handle as a JavaScript number (the id
/// `clearTimeout` takes back), which is also what the DOM lib's `setTimeout`
/// returns. So the handle's type is `number`: `let t: ReturnType<typeof
/// setTimeout> | null` is an `Option<f64>`, not an erased value. The
/// handle's methods (`ref`, `unref`, `refresh`) are not modeled, and a call
/// to one is refused by lowering.
fn is_node_timer_handle(symbol: &TsSymbolRef) -> bool {
    NODE_TIMER_HANDLES.contains(&symbol.qualified_name.as_str())
        && symbol
            .declaration
            .as_ref()
            .is_some_and(|declaration| declaration.file.contains("/node_modules/@types/node/"))
}

/// Whether `file` is one of the TypeScript lib declaration files.
fn is_lib_file(file: &str) -> bool {
    file.rsplit('/')
        .next()
        .is_some_and(|base| base.starts_with("lib.") && base.ends_with(".d.ts"))
        && !file.starts_with(PROGRAM_ROOT)
}

/// `path` as the checker bridge places it: relative to its program root.
fn program_relative(path: &str) -> String {
    normalize(
        &path
            .strip_prefix(PROGRAM_ROOT)
            .unwrap_or(path)
            .replace('\\', "/"),
    )
}

/// `module_path` as the bridge places it: canonical (the transpiler locates
/// program files at the canonical path of their source, so a file reached
/// through a symlink is the one file it names), normalized. A path that does
/// not exist (an in-memory source) is made absolute instead, as the bridge does.
fn module_program_relative(module_path: &str) -> String {
    let located = std::fs::canonicalize(module_path)
        .or_else(|_| std::path::absolute(module_path))
        .map_or_else(
            |_| module_path.to_owned(),
            |path| path.display().to_string(),
        );
    program_relative(&located)
}

/// Whether the checker's program `file` is the module at `module_path`.
fn same_file(file: &str, module_path: &str) -> bool {
    program_relative(file) == module_program_relative(module_path)
}

/// Whether the relative import `specifier`, written in `module_path`, names `file`.
fn specifier_resolves_to(module_path: &str, specifier: &str, file: &str) -> bool {
    if !specifier.starts_with('.') {
        return false;
    }
    let module = module_program_relative(module_path);
    let directory = module
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let joined = normalize(&format!("{directory}/{specifier}"));
    let target = program_relative(file);
    let base = joined.strip_suffix(".js").unwrap_or(&joined);
    [
        joined.clone(),
        format!("{base}.ts"),
        format!("{base}.tsx"),
        format!("{base}.d.ts"),
        format!("{base}/index.ts"),
        format!("{base}/index.d.ts"),
    ]
    .contains(&target)
}

/// Resolves `.` and `..` segments of a `/`-separated relative path.
fn normalize(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// A TypeScript string literal type spelling `text`.
fn string_literal(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(other));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// A number literal type spelling `value`; non-finite numbers have none.
fn number_literal(value: f64) -> Option<String> {
    value.is_finite().then(|| {
        if value < 0.0_f64 {
            format!("({value})")
        } else {
            value.to_string()
        }
    })
}
