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
//!   cannot reproduce).

use std::fmt::Write as _;

use oxc::allocator::Allocator;
use oxc::ast::ast::Statement;
use oxc::parser::Parser;
use oxc::span::SourceType;
use smelt_hir::{Item, Type, TypeId};
use smelt_ts_model::model::{TsObjectType, TsSymbolRef, TsTupleElement, TsTupleElementKind};
use smelt_ts_model::{TsTypeFacts, TsTypeId, TsTypeKind};

use super::ModuleBuilder;

/// Nesting depth beyond which a re-spelling gives up.
const MAX_DEPTH: usize = 12;

/// TypeScript's apparent-type root interfaces; see [`Respeller::named`].
const APPARENT_TYPE_ROOTS: [&str; 4] = ["Object", "Function", "CallableFunction", "NewableFunction"];

/// Virtual root the checker bridge places program files under (`smelt-ts-types`).
const PROGRAM_ROOT: &str = "/smelt/";

impl ModuleBuilder<'_> {
    /// The checker-evaluated type of the annotation at `start..end`, lowered
    /// through Smelt's annotation path; `None` when the checker has no
    /// faithful answer (see the module docs).
    pub(in crate::lowering) fn checker_annotation_type(&mut self, start: u32, end: u32) -> Option<TypeId> {
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
    pub(in crate::lowering) fn checker_binding_type(&mut self, start: u32, end: u32) -> Option<TypeId> {
        let facts = self.ts_facts?;
        let id = facts.type_id_at(&self.path, start, end)?;
        if let Some(TsTypeKind::Named { symbol, .. }) = facts.types.get(id).map(|ty| &ty.kind)
            && let Some(declared) = self.checker_declared_class_type(symbol)
        {
            return Some(declared);
        }
        self.respell_and_lower(facts, id, start, end)
    }

    /// Re-spells checker type `id` in this scope and lowers it; `None` for a
    /// type that is dynamic or unevaluated at the top or cannot be spelled.
    fn respell_and_lower(&mut self, facts: &TsTypeFacts, id: TsTypeId, start: u32, end: u32) -> Option<TypeId> {
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
        let text = Respeller { builder: self, facts, annotation: (start, end) }.respell(id, 0)?;
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
        let module = self.ctx.krate.modules.iter().find(|module| module_program_relative(&module.source.path) == target)?;
        let (name, type_params) = module.items.iter().find_map(|item| {
            let found = self.ctx.krate.items.get(usize::try_from(item.0).ok()?)?;
            let (name, type_params) = match found {
                Item::Class(class) => (class.name, &class.type_params),
                Item::Interface(interface) => (interface.name, &interface.type_params),
                Item::Function(_) | Item::TypeAlias(_) | Item::Const(_) | Item::MutableGlobal(_) => return None,
            };
            let original = self.ctx.krate.names.get(name).or_else(|| self.ctx.krate.symbols.source(name));
            (original == Some(symbol.name.as_str())).then(|| (name, type_params.clone()))
        })?;
        let args = type_params.iter().map(|param| param.default).collect::<Option<Vec<_>>>()?;
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
            return !self.types.module_declares_type_name(name) && self.imports.import_source(name).is_none();
        }
        if same_file(&declaration.file, &self.path) {
            return self.types.module_declares_type_name(name) || self.source_declares_type(name);
        }
        let Some(specifier) = self.imports.import_source(name) else {
            return false;
        };
        self.imports.imported_name(name).is_none_or(|imported| imported == name)
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
        keyword(&ty.kind).map(str::to_owned).or_else(|| self.structure(&ty.kind, next)).or_else(|| {
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
            same_file(&declaration.file, &self.builder.path) && declaration.start <= start && end <= declaration.end
        })
    }

    /// Spells literal and composite kinds.
    fn structure(&self, kind: &TsTypeKind, depth: usize) -> Option<String> {
        match kind {
            TsTypeKind::StringLiteral(text) => Some(string_literal(text)),
            TsTypeKind::NumberLiteral(value) => number_literal(*value),
            TsTypeKind::BooleanLiteral(value) => Some(value.to_string()),
            TsTypeKind::BigIntLiteral(value) => Some(format!("{value}n")),
            TsTypeKind::EnumMember { enum_symbol, .. } | TsTypeKind::Union { enum_symbol: Some(enum_symbol), .. } => {
                self.builder.type_name_visible(enum_symbol).then(|| enum_symbol.name.clone())
            }
            TsTypeKind::Union { members, enum_symbol: None } => self.joined(members, " | ", depth),
            TsTypeKind::Intersection(members) => self.joined(members, " & ", depth),
            TsTypeKind::TypeParameter { name, is_this, .. } => self.type_parameter(name, *is_this),
            TsTypeKind::Array { element, readonly } => {
                let element = self.respell(*element, depth)?;
                Some(format!("{}({element})[]", if *readonly { "readonly " } else { "" }))
            }
            TsTypeKind::Tuple { elements, readonly } => self.tuple(elements, *readonly, depth),
            TsTypeKind::Named { symbol, type_arguments } => {
                if self.builder.type_name_visible(symbol) {
                    self.named(&symbol.name, type_arguments, depth)
                } else {
                    None
                }
            }
            TsTypeKind::Object(object) => self.callable(object, depth),
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
        let parts = arguments.iter().map(|&argument| self.respell(argument, depth)).collect::<Option<Vec<_>>>()?;
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
        self.builder.type_parameter_type(name).map(|_| name.to_owned())
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
        Some(format!("{}[{}]", if readonly { "readonly " } else { "" }, parts.join(", ")))
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
    chars.next().is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|rest| rest.is_alphanumeric() || rest == '_' || rest == '$')
        && !name.starts_with("__")
}

/// Whether `file` is one of the TypeScript lib declaration files.
fn is_lib_file(file: &str) -> bool {
    file.rsplit('/').next().is_some_and(|base| base.starts_with("lib.") && base.ends_with(".d.ts"))
        && !file.starts_with(PROGRAM_ROOT)
}

/// `path` as the checker bridge places it: relative to its program root.
fn program_relative(path: &str) -> String {
    normalize(&path.strip_prefix(PROGRAM_ROOT).unwrap_or(path).replace('\\', "/"))
}

/// `module_path` as the bridge places it: absolute (the transpiler locates
/// program files at `std::path::absolute` of their source path), normalized.
///
/// `std::path::absolute` is pure path arithmetic against the working
/// directory; it does not touch the file system.
fn module_program_relative(module_path: &str) -> String {
    let absolute = std::path::absolute(module_path).map_or_else(|_| module_path.to_owned(), |path| path.display().to_string());
    program_relative(&absolute)
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
    let directory = module.rsplit_once('/').map_or("", |(directory, _)| directory);
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
    value.is_finite().then(|| if value < 0.0_f64 { format!("({value})") } else { value.to_string() })
}
