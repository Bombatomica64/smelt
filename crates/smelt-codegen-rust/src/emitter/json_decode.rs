//! Strict typed decoding of JSON at a source-declared entry boundary.
//!
//! Design adapted from Bun's `--aot` typed entry checks (oven-sh/bun#45631):
//! "a value that doesn't match its type throws a `TypeError` where it enters
//! typed code", and `Bun.validate<T>` / `Bun.parse<T>`, which derive the
//! validator from the TypeScript type instead of a hand-kept schema
//! (`docs/bundler/executables.mdx`, "Validate data by its type";
//! `src/aot/build/validate.rs`).
//!
//! # The boundary
//!
//! `JSON.parse(text)` is typed `any` by TypeScript, so the parsed value enters
//! typed code at the FIRST declared type it flows into: `JSON.parse<T>(s)`,
//! `JSON.parse(s) as T`, `const v: T = JSON.parse(s)`, a `T`-typed parameter or
//! a `T`-returning function. The frontend marks those parses
//! (`ExprKind::JsonParse { decode: true }`, MIR `BuiltinFn::JsonParse { target:
//! Some(T) }`). When `T` has a decodable Rust shape (see
//! [`FunctionEmitter::json_decode_target`]) the call site emits
//! `smelt_json_decode_<T>(&text)?`, which parses and then decodes the JSON
//! value into `T`'s concrete Rust type, throwing a catchable `TypeError` (an
//! error record `instanceof TypeError`) on the first mismatch.
//!
//! Behavioral requirement: for JSON that matches `T`, the decoded value is the
//! value Node produces (properties `T` does not declare are dropped — see
//! below). For JSON that does not match, Node silently carries the wrong value;
//! smelt throws `TypeError: expected <type>, got <value> at <path>` — an
//! intentional, Bun-aligned divergence documented in `docs/json-entry-decoding.md`.
//!
//! Type-preservation requirement: the decoded value is stored at `T`'s concrete
//! Rust type (`struct`, `SmeltUnion…` enum, `SmeltList<…>`, tuple,
//! `SmeltRecord<String, …>`, `Option<…>`, primitives). No `SmeltUnknown` is
//! kept after the boundary except where the SOURCE type itself is `unknown` (or
//! a position smelt already stores as a dynamic value), which is passed through
//! unchecked.
//!
//! # What is checked
//!
//! * `number` / `string` / `boolean` / `null` by JSON kind — no coercion, so
//!   `"3"` is not a `number`.
//! * Literal-typed properties (`kind: "circle"`, `mode: "a" | "b"`) by value,
//!   from the interface's `FieldDecodeFacts`.
//! * Properties: a missing required property is `got undefined`; `p?: T` and
//!   `p: T | undefined` may be missing; `null` is accepted only where the
//!   declared type admits `null` (properties without recorded facts — none
//!   are produced by declared interfaces — accept both, as their `Optional`
//!   storage does).
//! * Extra properties are dropped (the struct has no slot for them), as
//!   `Bun.validate` does.
//! * Arrays element-wise, tuples with exact arity, `Record<string, T>` /
//!   string index signatures value-wise.
//! * Unions arm by arm in smelt's canonical union order; the first arm that
//!   decodes wins. An arm rejected by a literal (discriminant) property is
//!   skipped when reporting, so `{kind: "square", side: "x"}` reports the
//!   `side` of the `Square` arm.
//! * Recursive interfaces through one decoder function per type.
//!
//! Types with no decodable shape — functions, classes (prototype identity),
//! generic instantiations, unresolved type parameters, `Date`, `Map`, `Set`,
//! symbols — make the whole target non-decodable, and the parse keeps the
//! existing dynamic boundary path unchanged.
//!
//! The decoders are emitted into the generated prelude next to the union
//! definitions: they are boundary adapters from the erased JSON value
//! (`SmeltUnknown`, the parser's output) into concrete storage.

use super::*;
use crate::rust::RustIdent;

/// Name of the per-type value decoder for a composite `ty`.
fn decoder_fn_name(ty: TypeId) -> String {
    format!("smelt_decode_{}", ty.0)
}

/// Name of the text-to-value entry adapter the call site invokes for `ty`.
pub(crate) fn json_decode_entry_name(ty: TypeId) -> String {
    format!("smelt_json_decode_{}", ty.0)
}

/// Render a literal as the Rust condition "`value_text` is this JSON value".
fn literal_condition(value_text: &str, literal: &smelt_hir::Literal) -> Option<String> {
    match literal {
        smelt_hir::Literal::String(text) => Some(format!(
            "matches!({value_text}, SmeltUnknown::String(text) if &**text == {text:?})"
        )),
        smelt_hir::Literal::Float(number) => Some(format!(
            "matches!({value_text}, SmeltUnknown::Number(number) if *number == {number:?}_f64)"
        )),
        // JSON has no bigint, so no parsed value is ever a bigint literal.
        smelt_hir::Literal::BigInt(_) => None,
        smelt_hir::Literal::Int(number) => Some(format!(
            "matches!({value_text}, SmeltUnknown::Number(number) if *number == {number}_f64)"
        )),
        smelt_hir::Literal::Bool(flag) => Some(format!(
            "matches!({value_text}, SmeltUnknown::Bool({flag}))"
        )),
        smelt_hir::Literal::Symbol(_) | smelt_hir::Literal::Undefined | smelt_hir::Literal::None => {
            None
        }
    }
}

/// Render a literal the way TypeScript spells it in a type (`"a"`, `3`, `true`).
fn literal_type_text(literal: &smelt_hir::Literal) -> String {
    match literal {
        smelt_hir::Literal::String(text) => format!("{text:?}"),
        smelt_hir::Literal::BigInt(digits) => format!("{digits}n"),
        smelt_hir::Literal::Float(number) => {
            if number.fract() == 0.0 && number.abs() < 1e21 {
                format!("{number:.0}")
            } else {
                format!("{number}")
            }
        }
        smelt_hir::Literal::Int(number) => number.to_string(),
        smelt_hir::Literal::Bool(flag) => flag.to_string(),
        smelt_hir::Literal::Symbol(_) => "symbol".to_owned(),
        smelt_hir::Literal::Undefined => "undefined".to_owned(),
        smelt_hir::Literal::None => "null".to_owned(),
    }
}

/// Decode facts for one property: what the source says beyond the storage type.
#[derive(Clone)]
struct PropertyFacts {
    /// The property may be missing from the JSON object.
    may_be_absent: bool,
    /// The property may be JSON `null`.
    admits_null: bool,
    /// Exact admitted values; empty for no value constraint.
    literals: Vec<smelt_hir::Literal>,
}

impl FunctionEmitter<'_> {
    /// The type a source-declared `JSON.parse` target decodes into, if any.
    ///
    /// `Some(ty)` when the parse carries a source-declared target AND that
    /// target has a decodable Rust shape; the call site then emits the strict
    /// decoder and the call's source type is `ty`. `None` keeps the dynamic
    /// parse (the erased value, coerced by the destination as before).
    pub(super) fn json_decode_target(&self, target: Option<TypeId>) -> Option<TypeId> {
        let declared = target?;
        self.json_decodable(declared, &mut Vec::new()).then_some(declared)
    }

    /// Whether `ty` has a Rust shape a strict decoder can build.
    ///
    /// `visiting` holds the interfaces on the current path, so a recursive
    /// interface (`interface Tree { children: Tree[] }`) answers by its
    /// non-recursive parts.
    fn json_decodable(&self, ty: TypeId, visiting: &mut Vec<TypeId>) -> bool {
        match self.mir.types.get(ty) {
            Some(
                Type::Bool
                | Type::Int
                | Type::FixedInt(_)
                | Type::Float
                | Type::String
                | Type::None
                | Type::Unknown,
            ) => true,
            // JSON has no bigint: `JSON.parse` can never produce one, so a
            // bigint target is not a decodable shape.
            Some(Type::BigInt) => false,
            Some(Type::List(item) | Type::Optional(item)) => self.json_decodable(*item, visiting),
            Some(Type::Tuple(items)) => items.iter().all(|item| self.json_decodable(*item, visiting)),
            Some(Type::Dict(key, value)) => {
                matches!(self.mir.types.get(*key), Some(Type::String))
                    && self.dict_uses_smelt_record(*key)
                    && self.json_decodable(*value, visiting)
            }
            Some(Type::Union(items)) => {
                items.len() >= 2 && items.iter().all(|item| self.json_decodable(*item, visiting))
            }
            Some(Type::Class { .. }) => {
                if visiting.contains(&ty) {
                    return true;
                }
                let Some(interface) = self.json_decode_interface(ty) else {
                    // A nested position smelt ALREADY stores as a dynamic
                    // value (an object-literal type alias such as
                    // `type Pair = { a: number }` names no generated struct)
                    // is passed through unchecked, exactly as the lenient
                    // path stores it, so one such property does not switch
                    // the checks off for the whole target. At the top level
                    // there is nothing to check, so the parse stays dynamic.
                    return !visiting.is_empty() && self.json_decode_passes_through(ty);
                };
                visiting.push(ty);
                let decodable = interface.fields.iter().all(|field| {
                    !matches!(self.mir.types.get(field.ty), Some(Type::Function(_)))
                        && self.json_decodable(field.ty, visiting)
                });
                visiting.pop();
                decodable
            }
            Some(
                Type::Never
                | Type::TypeParam { .. }
                | Type::Set(_)
                | Type::JsMap(_, _)
                | Type::Function(_)
                | Type::Future(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. },
            )
            | None => false,
        }
    }

    /// The plain-data interface a `Class` type names, when it is decodable as
    /// a struct: a declared (non-generic) interface, not a class — a class
    /// value has prototype identity and methods JSON cannot supply.
    fn json_decode_interface(&self, ty: TypeId) -> Option<&smelt_mir::MirInterface> {
        let Some(Type::Class { name, args }) = self.mir.types.get(ty) else {
            return None;
        };
        if !args.is_empty() || self.is_erased_class_type(ty) {
            return None;
        }
        if self.mir.classes.iter().any(|class| class.name == *name) {
            return None;
        }
        self.mir
            .interfaces
            .iter()
            .rev()
            .find(|interface| interface.name == *name)
            .filter(|interface| interface.type_params.is_empty())
    }

    /// Whether `ty` is a class-shaped type whose storage is already the dynamic
    /// value: no generated struct, class or modeled host type backs it, so
    /// the decoder keeps the parsed JSON for it as the lenient path would.
    fn json_decode_passes_through(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::Class { args, .. }) if args.is_empty())
            && self.is_erased_class_type(ty)
    }

    /// Describe `ty` the way TypeScript spells it, for `expected …` messages.
    fn json_decode_expected(&self, ty: TypeId) -> String {
        match self.mir.types.get(ty) {
            Some(Type::Bool) => "boolean".to_owned(),
            Some(Type::Int) => "integer".to_owned(),
            Some(Type::FixedInt(smelt_hir::IntWidth::I32)) => "int32".to_owned(),
            Some(Type::FixedInt(_)) => "integer".to_owned(),
            Some(Type::Float) => "number".to_owned(),
            Some(Type::String) => "string".to_owned(),
            Some(Type::None) => "null".to_owned(),
            Some(Type::Unknown) => "unknown".to_owned(),
            Some(Type::List(item)) => {
                let item_text = self.json_decode_expected(*item);
                if matches!(
                    self.mir.types.get(*item),
                    Some(Type::Union(_) | Type::Optional(_))
                ) {
                    format!("({item_text})[]")
                } else {
                    format!("{item_text}[]")
                }
            }
            Some(Type::Tuple(items)) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|item| self.json_decode_expected(*item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some(Type::Dict(_, value)) => {
                format!("Record<string, {}>", self.json_decode_expected(*value))
            }
            Some(Type::Optional(item)) => {
                format!("{} | null", self.json_decode_expected(*item))
            }
            Some(Type::Union(items)) => items
                .iter()
                .map(|item| self.json_decode_expected(*item))
                .collect::<Vec<_>>()
                .join(" | "),
            Some(Type::Class { name, .. }) => self
                .mir
                .names
                .get(*name)
                .or_else(|| self.mir.symbols.get(*name))
                .unwrap_or("object")
                .to_owned(),
            _ => "value".to_owned(),
        }
    }

    /// The expression decoding `value_text` (a `&SmeltUnknown`) at `ty`, as a
    /// `Result<RustT, SmeltDecodeIssue>`; composite types are queued in `deps`.
    fn json_decode_call(&self, ty: TypeId, value_text: &str, deps: &mut Vec<TypeId>) -> String {
        let primitive = match self.mir.types.get(ty) {
            Some(Type::Bool) => Some("smelt_decode_boolean"),
            Some(Type::Int) => Some("smelt_decode_integer"),
            Some(Type::FixedInt(smelt_hir::IntWidth::I32)) => Some("smelt_decode_int32"),
            Some(Type::FixedInt(smelt_hir::IntWidth::I64)) => Some("smelt_decode_integer"),
            Some(Type::FixedInt(smelt_hir::IntWidth::Usize)) => Some("smelt_decode_index"),
            Some(Type::Float) => Some("smelt_decode_number"),
            Some(Type::String) => Some("smelt_decode_string"),
            Some(Type::None) => Some("smelt_decode_null"),
            Some(Type::Unknown) => Some("smelt_decode_unknown"),
            Some(Type::Class { .. }) if self.json_decode_passes_through(ty) => {
                Some("smelt_decode_unknown")
            }
            _ => None,
        };
        if let Some(helper) = primitive {
            return format!("{helper}({value_text}, path)");
        }
        if !deps.contains(&ty) {
            deps.push(ty);
        }
        format!("{}({value_text}, path)", decoder_fn_name(ty))
    }

    /// Render the decoder function for one composite type.
    fn json_decoder_fn_text(&self, ty: TypeId, deps: &mut Vec<TypeId>) -> Result<String, EmitError> {
        let rust_ty = self.type_text_with_impl_trait(ty, false)?;
        let expected = self.json_decode_expected(ty);
        let body = match self.mir.types.get(ty).cloned() {
            Some(Type::List(item)) => {
                let item_call = self.json_decode_call(item, "&item", deps);
                format!(
                    "let SmeltUnknown::Array(items) = value else {{ return smelt_decode_fail(path, {expected:?}, value); }};\n    \
                     let mut decoded = Vec::with_capacity(items.len());\n    \
                     for (index, item) in items.iter().enumerate() {{ let mark = smelt_decode_enter_index(path, index); decoded.push({item_call}?); path.truncate(mark); }}\n    \
                     Ok(SmeltList::new(decoded))"
                )
            }
            Some(Type::Tuple(items)) => {
                let mut text = format!(
                    "let SmeltUnknown::Array(items) = value else {{ return smelt_decode_fail(path, {expected:?}, value); }};\n    \
                     if items.len() != {len} {{ return smelt_decode_arity_fail(path, {expected:?}, items.len()); }}\n",
                    len = items.len()
                );
                for (index, item) in items.iter().enumerate() {
                    let item_call = self.json_decode_call(*item, &format!("&item_{index}"), deps);
                    text.push_str(&format!(
                        "    let item_{index} = items.get({index}).unwrap_or(SmeltUnknown::Undefined);\n    \
                         let mark = smelt_decode_enter_index(path, {index}); let item_{index} = {item_call}?; path.truncate(mark);\n"
                    ));
                }
                let names = (0..items.len())
                    .map(|index| format!("item_{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                if items.len() == 1 {
                    text.push_str(&format!("    Ok(({names},))"));
                } else {
                    text.push_str(&format!("    Ok(({names}))"));
                }
                text
            }
            Some(Type::Dict(_, value_ty)) => {
                let item_call = self.json_decode_call(value_ty, "&item", deps);
                format!(
                    "let SmeltUnknown::Object(object) = value else {{ return smelt_decode_fail(path, {expected:?}, value); }};\n    \
                     let mut entries = Vec::with_capacity(object.len());\n    \
                     for (key, item) in object.iter() {{ let mark = smelt_decode_enter_key(path, &key); let decoded = {item_call}?; path.truncate(mark); entries.push((key, decoded)); }}\n    \
                     Ok(SmeltRecord::with_id_from_entries(smelt_next_object_id(), entries))"
                )
            }
            Some(Type::Optional(item)) => {
                let inner = self.flatten_optional_inner(item);
                let item_call = self.json_decode_call(inner, "other", deps);
                format!(
                    "let at = path.len();\n    match value {{ SmeltUnknown::Null => Ok(None), other => {item_call}.map(Some).map_err(|issue| smelt_decode_widen(issue, at, {expected:?})) }}"
                )
            }
            Some(Type::Union(items)) => {
                let concrete = self.concrete_union_members(ty).is_some();
                let mut text = "let mark = path.len();\n    let mut issues = Vec::new();\n".to_owned();
                for (index, item) in items.iter().enumerate() {
                    let item_call = self.json_decode_call(*item, "value", deps);
                    let success = if concrete {
                        format!("return Ok({}::M{index}(decoded))", union::union_name(ty))
                    } else {
                        // The union's storage is already the dynamic value
                        // (a member such as `null` keeps it out of the tagged
                        // enum), so the arm only VALIDATES and the JSON value
                        // is kept as stored.
                        "{ let _ = decoded; return Ok(value.clone()) }".to_owned()
                    };
                    text.push_str(&format!(
                        "    match {item_call} {{ Ok(decoded) => {success}, Err(issue) => {{ path.truncate(mark); issues.push(issue); }} }}\n"
                    ));
                }
                text.push_str(&format!(
                    "    smelt_decode_union_fail(path, {expected:?}, value, issues)"
                ));
                text
            }
            Some(Type::Class { .. }) => self.json_decoder_struct_body(ty, &expected, deps)?,
            _ => {
                return Err(EmitError::new(
                    "a JSON decoder was requested for a type with no decodable shape",
                ));
            }
        };
        Ok(format!(
            "\n/// Strict JSON decoder for `{expected}`.\nfn {name}(value: &SmeltUnknown, path: &mut String) -> Result<{rust_ty}, SmeltDecodeIssue> {{\n    {body}\n}}\n",
            name = decoder_fn_name(ty)
        ))
    }

    /// The facts a property decoder checks, from the interface's recorded
    /// `FieldDecodeFacts` or, when none were recorded, from its storage type.
    fn json_property_facts(
        &self,
        interface: &smelt_mir::MirInterface,
        field: &MirField,
    ) -> PropertyFacts {
        let optional = matches!(self.mir.types.get(field.ty), Some(Type::Optional(_)));
        interface
            .decode_facts
            .iter()
            .find(|facts| facts.name == field.name)
            .map_or(
                PropertyFacts {
                    may_be_absent: optional,
                    admits_null: optional,
                    literals: Vec::new(),
                },
                |facts| PropertyFacts {
                    may_be_absent: facts.may_be_absent && optional,
                    admits_null: facts.admits_null && optional,
                    literals: facts.literals.clone(),
                },
            )
    }

    /// Render the body of an interface (struct) decoder.
    fn json_decoder_struct_body(
        &self,
        ty: TypeId,
        expected: &str,
        deps: &mut Vec<TypeId>,
    ) -> Result<String, EmitError> {
        let interface = self
            .json_decode_interface(ty)
            .ok_or_else(|| EmitError::new("JSON decoder interface is missing"))?;
        let struct_name = self.type_text_with_impl_trait(ty, false)?;
        let mut text = format!(
            "let SmeltUnknown::Object(object) = value else {{ return smelt_decode_fail(path, {expected:?}, value); }};\n"
        );
        let mut inits = Vec::new();
        for (index, field) in interface.fields.iter().enumerate() {
            let rust_field = RustIdent::new(self.mir.symbols.get(field.name).unwrap_or("field"))
                .into_string();
            let key = self
                .mir
                .names
                .get(field.name)
                .or_else(|| self.mir.symbols.get(field.name))
                .unwrap_or("field");
            let facts = self.json_property_facts(interface, field);
            let inner_ty = match self.mir.types.get(field.ty) {
                Some(Type::Optional(inner)) => Some(self.flatten_optional_inner(*inner)),
                _ => None,
            };
            let value_ty = inner_ty.unwrap_or(field.ty);
            let mut value_expected = if facts.literals.is_empty() {
                self.json_decode_expected(value_ty)
            } else {
                facts
                    .literals
                    .iter()
                    .map(literal_type_text)
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            if facts.admits_null {
                value_expected.push_str(" | null");
            }
            if facts.may_be_absent {
                value_expected.push_str(" | undefined");
            }
            let literal_check = {
                let conditions = facts
                    .literals
                    .iter()
                    .filter_map(|literal| literal_condition("&field", literal))
                    .collect::<Vec<_>>();
                if conditions.is_empty() {
                    String::new()
                } else {
                    format!(
                        "if !({}) {{ return smelt_decode_literal_fail(path, {value_expected:?}, &field); }} ",
                        conditions.join(" || ")
                    )
                }
            };
            let call = self.json_decode_call(value_ty, "&field", deps);
            // Restate an issue raised AT this property with the property's
            // declared type only when that type says more than the storage
            // type (`| null`, `| undefined`, a literal union).
            let decode = if value_expected == self.json_decode_expected(value_ty) {
                format!("{call}?")
            } else {
                format!(
                    "{call}.map_err(|issue| smelt_decode_widen(issue, at, {value_expected:?}))?"
                )
            };
            let present = if inner_ty.is_some() {
                format!("{literal_check}Some({decode})")
            } else {
                format!("{literal_check}{decode}")
            };
            let absent = if facts.may_be_absent {
                "None".to_owned()
            } else {
                format!(
                    "return smelt_decode_fail(path, {value_expected:?}, &SmeltUnknown::Undefined)"
                )
            };
            let null_arm = if facts.admits_null {
                "Some(SmeltUnknown::Null) => None, ".to_owned()
            } else {
                String::new()
            };
            text.push_str(&format!(
                "    let mark = smelt_decode_enter_key(path, {key:?});\n    let at = path.len();\n    \
                 let field_{index} = match object.get({key:?}) {{ {null_arm}Some(field) => {{ {present} }}, None => {absent} }};\n    \
                 path.truncate(mark);\n"
            ));
            inits.push(format!("{rust_field}: field_{index}"));
        }
        text.push_str(&format!("    Ok({struct_name} {{ {} }})", inits.join(", ")));
        Ok(text)
    }

    /// Render the text entry adapter `smelt_json_decode_<ty>` for one target.
    fn json_decode_entry_fn_text(&self, ty: TypeId, deps: &mut Vec<TypeId>) -> Result<String, EmitError> {
        let rust_ty = self.type_text_with_impl_trait(ty, false)?;
        let expected = self.json_decode_expected(ty);
        let decode = self.json_decode_call(ty, "&value", deps);
        Ok(format!(
            "\n/// `JSON.parse` into `{expected}`: parse, then decode strictly, throwing a catchable `TypeError` on a mismatch.\n\
             fn {name}(text: &str) -> Result<{rust_ty}, Box<dyn ::std::error::Error>> {{\n    \
             let value = {parse}(text)?;\n    \
             let path = &mut String::new();\n    \
             {decode}.map_err(smelt_decode_throw)\n}}\n",
            name = json_decode_entry_name(ty),
            parse = crate::thrown::JSON_PARSE_FN,
        ))
    }
}

/// Every decode target named by a `JSON.parse` call in the program.
fn json_decode_targets(mir: &Mir) -> Vec<TypeId> {
    let mut targets = Vec::new();
    let bodies = mir
        .functions
        .iter()
        .map(|function| (&function.blocks, &function.locals))
        .chain(mir.closures.iter().map(|closure| (&closure.blocks, &closure.locals)));
    for (blocks, locals) in bodies {
        for block in blocks {
            if let Some(Terminator::Call {
                callee: Callee::Builtin(BuiltinFn::JsonParse { target: Some(target) }),
                ..
            }) = &block.terminator
                && !targets.contains(target)
            {
                targets.push(*target);
            }
            // A `json()` body read the frontend retyped `Promise<T>` (see
            // `adopt_json_body_read_target`) decodes into `T` as well.
            for statement in &block.statements {
                let Statement::Assign {
                    dest,
                    value:
                        Rvalue::ResponseOp {
                            op: smelt_hir::ResponseOp::Json,
                            ..
                        }
                        | Rvalue::RequestOp {
                            op: smelt_hir::RequestOp::Json,
                            ..
                        },
                } = statement
                else {
                    continue;
                };
                let Some(local) = usize::try_from(dest.0).ok().and_then(|index| locals.get(index))
                else {
                    continue;
                };
                if let Some(Type::Future(target)) = mir.types.get(local.ty)
                    && !targets.contains(target)
                {
                    targets.push(*target);
                }
            }
        }
    }
    targets.sort_by_key(|target| target.0);
    targets
}

impl FunctionEmitter<'_> {
    /// The type a `Response`/`Request` `json()` read decodes into, when its
    /// destination is `Promise<T>` for a decodable `T`.
    ///
    /// Only the frontend's declared-target rule
    /// (`adopt_json_body_read_target`) types a `json()` read anything but
    /// `Promise<unknown>`, so a `Promise<T>` destination is a source-declared
    /// entry boundary.
    pub(super) fn json_body_decode_target(&self, dest_ty: TypeId) -> Option<TypeId> {
        let Some(Type::Future(target)) = self.mir.types.get(dest_ty) else {
            return None;
        };
        self.json_decode_target(Some(*target))
    }

    /// Render a `json()` body read that decodes strictly into `target`.
    ///
    /// The same handle-clone-into-the-future shape as the untyped reader in
    /// `fetch_types` (`text()` then the fallible parse), with the typed entry
    /// adapter in place of the dynamic parse, so a malformed body still rejects
    /// with a `SyntaxError` and a mismatching one with a `TypeError`.
    pub(super) fn json_body_decode_text(
        &self,
        receiver: &Operand,
        args: &[Operand],
        target: TypeId,
    ) -> Result<String, EmitError> {
        if !args.is_empty() {
            return Err(EmitError::new("`json()` takes no arguments"));
        }
        let receiver_text = self.operand_text(receiver)?;
        Ok(format!(
            "{{ let smelt_body_owner = {receiver_text}.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_text = smelt_body_owner.take_text()?; {decode}(&smelt_text) }})) }}",
            decode = json_decode_entry_name(target)
        ))
    }
}

/// Emit the decode support helpers, every needed entry adapter and every
/// per-type decoder, in deterministic order; empty when no `JSON.parse` has a
/// decodable declared target.
pub(crate) fn emit_json_decoders(mir: &Mir, context: &EmitContext) -> Result<String, EmitError> {
    let Some(function) = mir.functions.first() else {
        return Ok(String::new());
    };
    let emitter = FunctionEmitter::new(mir, context, function)?;
    let targets = json_decode_targets(mir)
        .into_iter()
        .filter(|target| emitter.json_decode_target(Some(*target)).is_some())
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(String::new());
    }
    let mut output = json_decode_support_text();
    let mut deps = Vec::new();
    for target in &targets {
        output.push_str(&emitter.json_decode_entry_fn_text(*target, &mut deps)?);
    }
    let mut written = Vec::new();
    while let Some(ty) = deps.iter().copied().find(|ty| !written.contains(ty)) {
        written.push(ty);
        output.push_str(&emitter.json_decoder_fn_text(ty, &mut deps)?);
    }
    Ok(output)
}

/// The shared decode runtime: the issue type, path bookkeeping, primitive
/// decoders and the `TypeError` conversion.
fn json_decode_support_text() -> String {
    let throw = crate::thrown::THROW_FN;
    let record = crate::thrown::error_payload_record_expr("TypeError", "message");
    format!(
        r#"/// One mismatch found by a strict JSON decoder: where, what the declared type expected, and what the JSON held.
struct SmeltDecodeIssue {{ path: String, expected: String, received: String, discriminant: bool }}

/// Describe a JSON value for an `expected …, got …` message.
fn smelt_decode_received(value: &SmeltUnknown) -> String {{
    match value {{
        SmeltUnknown::Null => "null".to_owned(),
        SmeltUnknown::Undefined => "undefined".to_owned(),
        SmeltUnknown::Bool(flag) => format!("boolean {{flag}}"),
        SmeltUnknown::Number(number) => if number.fract() == 0.0 && number.abs() < 1e21 {{ format!("number {{number:.0}}") }} else {{ format!("number {{number}}") }},
        SmeltUnknown::String(text) => {{ let shown: String = text.chars().take(40).collect(); let quoted = serde_json::to_string(&shown).unwrap_or_default(); if text.chars().count() > 40 {{ format!("string {{}}...", quoted) }} else {{ format!("string {{quoted}}") }} }}
        SmeltUnknown::Array(_) => "array".to_owned(),
        SmeltUnknown::Object(_) => "object".to_owned(),
        _ => "value".to_owned(),
    }}
}}

/// Fail at the current path with a kind mismatch.
fn smelt_decode_fail<T>(path: &str, expected: &str, value: &SmeltUnknown) -> Result<T, SmeltDecodeIssue> {{
    Err(SmeltDecodeIssue {{ path: path.to_owned(), expected: expected.to_owned(), received: smelt_decode_received(value), discriminant: false }})
}}

/// Fail at the current path because a literal-typed property held another value.
fn smelt_decode_literal_fail<T>(path: &str, expected: &str, value: &SmeltUnknown) -> Result<T, SmeltDecodeIssue> {{
    Err(SmeltDecodeIssue {{ path: path.to_owned(), expected: expected.to_owned(), received: smelt_decode_received(value), discriminant: true }})
}}

/// Fail because a tuple held the wrong number of elements.
fn smelt_decode_arity_fail<T>(path: &str, expected: &str, len: usize) -> Result<T, SmeltDecodeIssue> {{
    Err(SmeltDecodeIssue {{ path: path.to_owned(), expected: expected.to_owned(), received: format!("array of length {{len}}"), discriminant: false }})
}}

/// Restate an issue raised at exactly the position whose path is `at` bytes long with the wider declared type (`T | null`).
///
/// Compared by length because a nested decoder that failed returned early without truncating the shared path; an issue from a deeper position always has a longer path.
fn smelt_decode_widen(mut issue: SmeltDecodeIssue, at: usize, expected: &str) -> SmeltDecodeIssue {{
    if issue.path.len() == at {{ expected.clone_into(&mut issue.expected); }}
    issue
}}

/// Report a union no arm accepted: the deepest issue of an arm whose discriminant matched, else the union itself.
fn smelt_decode_union_fail<T>(path: &str, expected: &str, value: &SmeltUnknown, issues: Vec<SmeltDecodeIssue>) -> Result<T, SmeltDecodeIssue> {{
    let mut candidates = issues.into_iter().filter(|issue| !issue.discriminant && issue.path.len() > path.len()).collect::<Vec<_>>();
    if candidates.len() == 1 {{ return Err(candidates.remove(0)); }}
    smelt_decode_fail(path, expected, value)
}}

/// Append `.key` (or `["key"]`) to the path, answering the length to truncate back to.
fn smelt_decode_enter_key(path: &mut String, key: &str) -> usize {{
    let mark = path.len();
    let plain = key.chars().next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$') && key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$');
    if plain {{ path.push('.'); path.push_str(key); }} else {{ path.push('['); path.push_str(&serde_json::to_string(key).unwrap_or_default()); path.push(']'); }}
    mark
}}

/// Append `[index]` to the path, answering the length to truncate back to.
fn smelt_decode_enter_index(path: &mut String, index: usize) -> usize {{
    let mark = path.len();
    path.push_str(&format!("[{{index}}]"));
    mark
}}

/// Decode a JSON number (no string or boolean coercion).
fn smelt_decode_number(value: &SmeltUnknown, path: &mut String) -> Result<f64, SmeltDecodeIssue> {{
    match value {{ SmeltUnknown::Number(number) => Ok(*number), other => smelt_decode_fail(path, "number", other) }}
}}

/// Decode a JSON number that is a whole number in `i64` range.
fn smelt_decode_integer(value: &SmeltUnknown, path: &mut String) -> Result<i64, SmeltDecodeIssue> {{
    match value {{ SmeltUnknown::Number(number) if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 => Ok(*number as i64), other => smelt_decode_fail(path, "integer", other) }}
}}

/// Decode a JSON number that is Bun's `int32`: a whole number from -2^31 to
/// 2^31 - 1. `-0` is rejected: an `i32` slot cannot hold it, so accepting it
/// would silently turn `Object.is(v, -0)` false.
fn smelt_decode_int32(value: &SmeltUnknown, path: &mut String) -> Result<i32, SmeltDecodeIssue> {{
    match value {{ SmeltUnknown::Number(number) if number.fract() == 0.0 && *number >= -2_147_483_648.0 && *number <= 2_147_483_647.0 && !(*number == 0.0 && number.is_sign_negative()) => Ok(*number as i32), other => smelt_decode_fail(path, "int32", other) }}
}}

/// Decode a JSON number that is a whole, non-negative number below 2^53 (a
/// `usize` slot proven never negative).
fn smelt_decode_index(value: &SmeltUnknown, path: &mut String) -> Result<usize, SmeltDecodeIssue> {{
    match value {{ SmeltUnknown::Number(number) if number.fract() == 0.0 && *number >= 0.0 && *number < 9_007_199_254_740_992.0 && !number.is_sign_negative() => Ok(*number as usize), other => smelt_decode_fail(path, "integer", other) }}
}}

/// Decode a JSON string.
fn smelt_decode_string(value: &SmeltUnknown, path: &mut String) -> Result<String, SmeltDecodeIssue> {{
    match value {{ SmeltUnknown::String(text) => Ok(text.to_string()), other => smelt_decode_fail(path, "string", other) }}
}}

/// Decode a JSON boolean.
fn smelt_decode_boolean(value: &SmeltUnknown, path: &mut String) -> Result<bool, SmeltDecodeIssue> {{
    match value {{ SmeltUnknown::Bool(flag) => Ok(*flag), other => smelt_decode_fail(path, "boolean", other) }}
}}

/// Decode JSON `null`.
fn smelt_decode_null(value: &SmeltUnknown, path: &mut String) -> Result<(), SmeltDecodeIssue> {{
    match value {{ SmeltUnknown::Null => Ok(()), other => smelt_decode_fail(path, "null", other) }}
}}

/// A position the SOURCE typed `unknown`: the JSON value is kept as parsed (a genuine dynamic boundary).
fn smelt_decode_unknown(value: &SmeltUnknown, _path: &mut String) -> Result<SmeltUnknown, SmeltDecodeIssue> {{
    Ok(value.clone())
}}

/// Throw a decode issue as a catchable `TypeError` (`expected …, got … at …`).
fn smelt_decode_throw(issue: SmeltDecodeIssue) -> Box<dyn ::std::error::Error> {{
    let message = if issue.path.is_empty() {{ format!("expected {{}}, got {{}}", issue.expected, issue.received) }} else {{ format!("expected {{}}, got {{}} at {{}}", issue.expected, issue.received, issue.path) }};
    {throw}({record})
}}
"#
    )
}
