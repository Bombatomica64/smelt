//! Primitive casts: `Number(x)`, `String(x)`, `Boolean(x)` and their list forms,
//! at each source type.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Converts a primitive source-language cast operation to Rust text.
    /// `String.fromCharCode` / `String.fromCodePoint` over rendered list text
    /// (a `SmeltList<f64>` of codes).
    ///
    /// `fromCharCode`: each number through `ToUint16` (truncate, wrap modulo
    /// 2^16, non-finite is 0) as one UTF-16 code unit; a lone surrogate has no
    /// Rust `String` spelling and decodes to U+FFFD. `fromCodePoint`: each
    /// number is one code point; the spec's `RangeError` for a non-integer or
    /// out-of-range value becomes U+FFFD here.
    fn primitive_cast_list_expr(
        &self,
        op: smelt_hir::PrimitiveCastOp,
        list_text: &RustExpr,
        _operand_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match op {
            // A JavaScript string holds the code units as given, lone halves
            // included; a Rust `String` cannot, so it replaces them.
            smelt_hir::PrimitiveCastOp::FromCharCodes => Ok(RustExpr::atom(format!(
                "{{ let smelt_units: Vec<u16> = {list_text}.borrow().iter().map(|smelt_code| {{ let smelt_code = *smelt_code; if smelt_code.is_finite() {{ smelt_code.trunc().rem_euclid(65536.0) as u16 }} else {{ 0 }} }}).collect(); {} }}",
                if self.indexes_utf16() { "JsString::from(smelt_units)" } else { "String::from_utf16_lossy(&smelt_units)" }
            ))),
            smelt_hir::PrimitiveCastOp::FromCodePoints => Ok(RustExpr::atom_on(&list_text, format!(
                "{list_text}.borrow().iter().map(|smelt_code| {{ let smelt_code = *smelt_code; if smelt_code.fract() == 0.0 && (0.0..=1_114_111.0).contains(&smelt_code) {{ char::from_u32(smelt_code as u32).unwrap_or(char::REPLACEMENT_CHARACTER) }} else {{ char::REPLACEMENT_CHARACTER }} }}).collect::<String>()"
            ))),
            _ => Err(EmitError::new("not a code-list conversion")),
        }
    }

    /// Converts a primitive cast to Rust text.
    ///
    /// The arms of [`Self::primitive_cast_rust_expr`] build a Rust `String`
    /// for most string results; a JavaScript string destination takes that
    /// text through `JsString::from`. The arms that already answer this
    /// body's string type (a string operand, and everything
    /// `string_like_operand_expr` renders) are left as they are.
    pub(super) fn primitive_cast_expr(
        &self,
        op: smelt_hir::PrimitiveCastOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let to_js_text = self.mir.types.get(dest_ty) == Some(&Type::String);
        let operand_ty = self.operand_ty(operand)?;
        if to_js_text
            && op == smelt_hir::PrimitiveCastOp::ToString
            && let Some(Type::Optional(inner)) = self.mir.types.get(operand_ty)
            && self.mir.types.get(*inner) == Some(&Type::String)
        {
            let operand_expr = self.operand_expr(operand)?;
            return Ok(RustExpr::postfix_on(&operand_expr, format!(
                "{operand_expr}.unwrap_or_else(|| {})",
                RustExpr::js_string_literal(self.absent_spelling().text())
            )));
        }
        let text = self.primitive_cast_rust_expr(op, operand, dest_ty)?;
        let answers_rust_string = match op {
            smelt_hir::PrimitiveCastOp::FromCharCodes => !self.indexes_utf16(),
            smelt_hir::PrimitiveCastOp::FromCodePoints => true,
            smelt_hir::PrimitiveCastOp::ToString => match self.mir.types.get(operand_ty) {
                Some(Type::Bool | Type::Int | Type::Float | Type::FixedInt(_) | Type::BigInt | Type::PyStr) => true,
                Some(Type::Optional(inner)) => matches!(self.mir.types.get(*inner), Some(Type::Bool | Type::Int | Type::Float | Type::PyStr)),
                _ => self.date_handle_expr(&RustExpr::path("smelt_date"), operand_ty).is_some(),
            },
            _ => false,
        };
        if to_js_text && answers_rust_string {
            return Ok(RustExpr::chain(format!("JsString::from({text})")));
        }
        Ok(text)
    }

    /// [`Self::primitive_cast_expr`] before a JavaScript string destination
    /// is given its type.
    fn primitive_cast_rust_expr(
        &self,
        op: smelt_hir::PrimitiveCastOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        let operand_type = self
            .mir
            .types
            .get(operand_ty)
            .ok_or_else(|| EmitError::new("primitive cast operand has unknown type"))?;
        let dest_type = self
            .mir
            .types
            .get(dest_ty)
            .ok_or_else(|| EmitError::new("primitive cast destination has unknown type"))?;
        // A concrete union stores a tagged `SmeltUnion…` enum, but the erased JS
        // coercion arms below (`Type::Union(_)`) match over `SmeltUnknown`
        // discriminants. Project a concrete-union operand back to its erased value
        // so those arms see the `SmeltUnknown` shape they expect. Non-concrete
        // unions are already stored erased, so this is a no-op for them.
        let operand_expr = self.erase_concrete_union_expr(&self.operand_expr(operand)?, operand_ty);
        if let Some(text) = self.fixed_int_cast_expr(op, dest_type, operand_type, &operand_expr) {
            return Ok(text);
        }
        // `String.fromCharCode(...view)` / `fromCodePoint(...view)` over a
        // typed array: the view's elements, as numbers, are the code list, so
        // the list arms below apply to them unchanged.
        if matches!(
            op,
            smelt_hir::PrimitiveCastOp::FromCharCodes | smelt_hir::PrimitiveCastOp::FromCodePoints
        ) && matches!(dest_type, Type::String | Type::PyStr)
            && self.is_typed_array_view_class_type(operand_ty)?
        {
            let elements = RustExpr::atom(format!("SmeltList::from({operand_expr}.to_elements())"));
            return self.primitive_cast_list_expr(op, &elements, operand_ty);
        }
        // A concrete `Date` (`SmeltDate`) converts through its time value:
        // `+d` / `d.getTime()` is the number, `String(d)` the date string, and
        // a date object is always truthy.
        if let Some(handle) = self.date_handle_expr(&operand_expr, operand_ty) {
            match (op, dest_type) {
                (
                    smelt_hir::PrimitiveCastOp::ToFloat | smelt_hir::PrimitiveCastOp::ToJsNumber,
                    Type::Float,
                ) => return Ok(RustExpr::postfix_on(&handle, format!("{handle}.get_time()"))),
                (smelt_hir::PrimitiveCastOp::ToInt, Type::Int) => {
                    return Ok(RustExpr::opaque(format!("{handle}.get_time() as i64")));
                }
                (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool) => return Ok(RustExpr::path("true")),
                (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr) => {
                    return self.date_to_string_expr(operand);
                }
                _ => {}
            }
        }
        // `Number(d)` over a possibly-absent date: `undefined` is `NaN`.
        if let Type::Optional(inner) = operand_type
            && let Some(handle) = self.date_handle_expr(&RustExpr::path("smelt_date"), *inner)
            && matches!(
                (op, dest_type),
                (
                    smelt_hir::PrimitiveCastOp::ToFloat | smelt_hir::PrimitiveCastOp::ToJsNumber,
                    Type::Float
                )
            )
        {
            return Ok(RustExpr::atom(format!(
                "({operand_expr}).as_ref().map_or(f64::NAN, |smelt_date| {handle}.get_time())"
            )));
        }
        // A bigint converts without ever being rounded through anything but its
        // own nearest `f64` (`Number(2n ** 64n)`), and prints without its `n`
        // (`String(10n)` is `"10"`).
        if matches!(operand_type, Type::BigInt) {
            let value = self.bigint_ref_expr(operand)?;
            match (op, dest_type) {
                (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool) => {
                    return Ok(RustExpr::loose(format!("!::num_traits::Zero::is_zero({value})")));
                }
                (
                    smelt_hir::PrimitiveCastOp::ToFloat | smelt_hir::PrimitiveCastOp::ToJsNumber,
                    Type::Float,
                ) => return Ok(RustExpr::chain(format!("smelt_bigint_to_number({value})"))),
                (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr) => {
                    return Ok(RustExpr::atom(format!("({value}).to_string()")));
                }
                _ => {}
            }
        }
        match (op, dest_type, operand_type) {
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Bool)
            | (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Int)
            | (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Float)
            | (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Float)
            | (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, Type::String | Type::PyStr) => {
                Ok(operand_expr)
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Int) => {
                Ok(RustExpr::loose(format!("{operand_expr} != 0")))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Float) => Ok(RustExpr::atom(format!(
                "{{ let smelt_number = {operand_expr}; smelt_number != 0.0 && !smelt_number.is_nan() }}"
            ))),
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::String | Type::PyStr) => {
                Ok(RustExpr::loose(format!("!{operand_expr}.is_empty()")))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Union(_))
                if self.union_is_constantly_truthy(operand_ty) =>
            {
                Ok(RustExpr::path("true"))
            }
            (
                smelt_hir::PrimitiveCastOp::ToBool,
                Type::Bool,
                Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never,
            ) => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {operand_expr} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), {smelt_bigint_arm}SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => !::num_traits::Zero::is_zero(&value),")
            )))),
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Class { .. }) => {
                Ok(RustExpr::path("true"))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Optional(inner)) => {
                self.optional_truthy_expr(&operand_expr, *inner)
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Function(_)) => {
                Ok(RustExpr::path("true"))
            }
            // Every object is truthy, collections included: `!obj` on a
            // `Record<K, V>` (rendered as a map) is `false`, never `true`.
            // These operands used to reach the `(_, Type::Bool, _)` default
            // below and read as `false`, so `if (!obj) return {}` returned
            // early for every map. Same rule as `value_truthy_expr`.
            (
                smelt_hir::PrimitiveCastOp::ToBool,
                Type::Bool,
                Type::List(_)
                | Type::Tuple(_)
                | Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Set(_)
                | Type::Future(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. },
            ) => Ok(RustExpr::path("true")),
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Bool) => {
                Ok(RustExpr::loose(format!("if {operand_expr} {{ 1_i64 }} else {{ 0_i64 }}")))
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Float) => {
                Ok(RustExpr::loose(format!("({operand_expr} as f64).trunc() as i64")))
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::String | Type::PyStr) => Ok(RustExpr::atom_on(&operand_expr, format!(
                "{operand_expr}.parse::<i64>().expect(\"int() parse failed\")"
            ))),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Bool) => {
                Ok(RustExpr::loose(format!("if {operand_expr} {{ 1.0 }} else {{ 0.0 }}")))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Int) => {
                Ok(RustExpr::atom(format!("({operand_expr} as f64)")))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::String | Type::PyStr) => Ok(RustExpr::atom_on(&operand_expr, format!(
                "{operand_expr}.parse::<f64>().expect(\"float() parse failed\")"
            ))),
            (
                smelt_hir::PrimitiveCastOp::FromCharCodes | smelt_hir::PrimitiveCastOp::FromCodePoints,
                Type::String | Type::PyStr,
                Type::List(_),
            ) => self.primitive_cast_list_expr(op, &operand_expr, operand_ty),
            (smelt_hir::PrimitiveCastOp::ParseFloat, Type::Float, Type::String | Type::PyStr) => Ok(RustExpr::chain(format!(
                "smelt_parse_float(&{operand_expr})"
            ))),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Float)) =>
            {
                Ok(RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.unwrap_or(f64::NAN)")))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Int)) =>
            {
                Ok(RustExpr::postfix_on(&operand_expr, format!(
                    "{operand_expr}.map_or(f64::NAN, |value| value as f64)"
                )))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)
                ) || self.is_erased_class_type(*inner) =>
            {
                Ok(RustExpr::postfix_on(&operand_expr, self.carrier_format(format_args!(
                    "{operand_expr}.map_or(f64::NAN, |value| match value {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => smelt_js_string_to_number(&value), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }})", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),")
                ))))
            }
            (
                smelt_hir::PrimitiveCastOp::ToFloat,
                Type::Float,
                Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never,
            ) => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {operand_expr} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => smelt_js_string_to_number(&value), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),")
            )))),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Class { .. })
                if self.is_erased_class_type(operand_ty) =>
            {
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {operand_expr} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => smelt_js_string_to_number(&value), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),")
                ))))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Bool) => {
                Ok(RustExpr::loose(format!("if {operand_expr} {{ 1.0 }} else {{ 0.0 }}")))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Int) => {
                Ok(RustExpr::atom(format!("({operand_expr} as f64)")))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::String | Type::PyStr) => {
                Ok(self.js_number_from_string_expr(&operand_expr))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Float)) =>
            {
                Ok(RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.unwrap_or(f64::NAN)")))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Int)) =>
            {
                Ok(RustExpr::postfix_on(&operand_expr, format!(
                    "{operand_expr}.map_or(f64::NAN, |value| value as f64)"
                )))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::String | Type::PyStr)) =>
            {
                let conversion = self.js_number_from_string_expr(&RustExpr::path("value"));
                Ok(RustExpr::postfix_on(&operand_expr, format!(
                    "{operand_expr}.map_or(f64::NAN, |value| {conversion})"
                )))
            }
            (
                smelt_hir::PrimitiveCastOp::ToJsNumber,
                Type::Float,
                Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never,
            ) => Ok(self.js_number_from_unknown_expr(&operand_expr)),
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)
                ) || self.is_erased_class_type(*inner) =>
            {
                let conversion = self.js_number_from_unknown_expr(&RustExpr::path("value"));
                Ok(RustExpr::postfix_on(&operand_expr, format!(
                    "{operand_expr}.map_or(f64::NAN, |value| {conversion})"
                )))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Class { .. })
                if self.is_erased_class_type(operand_ty) =>
            {
                Ok(self.js_number_from_unknown_expr(&operand_expr))
            }
            (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, Type::Bool) => Ok(RustExpr::loose(format!(
                "if {operand_expr} {{ \"True\".to_owned() }} else {{ \"False\".to_owned() }}"
            ))),
            // `String(x)` and `x.toString()` on a NUMBER follow JavaScript's
            // own rule, which parts company with Rust's `Display` at both ends
            // of the range (`1e+21`, `1e-7`). An `Int` is still a JavaScript
            // number, and a JS number is an `f64`, so both widths go through
            // the one helper rather than depending on which Smelt inferred.
            // Python's `str(int)` is the exact decimal digits and `str(float)`
            // is `repr(float)` (`12.0`); a body's language decides.
            (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, Type::Int)
                if self.absent_spelling() == AbsentSpelling::None =>
            {
                Ok(RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.to_string()")))
            }
            (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, Type::Float)
                if self.absent_spelling() == AbsentSpelling::None =>
            {
                Ok(RustExpr::chain(format!(
                    "{fn_name}({operand_expr})",
                    fn_name = smelt_stdlib::runtime_symbols::number_text::PY_FLOAT_REPR_FN,
                )))
            }
            (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, Type::Int) => Ok(RustExpr::chain(format!(
                "{fn_name}({operand_expr} as f64)",
                fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
            ))),
            (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, Type::Float) => Ok(RustExpr::chain(format!(
                "{fn_name}({operand_expr})",
                fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
            ))),
            // `String(x)` on an absent optional is the language's absent WORD,
            // not the empty string. `unwrap_or_default()` answered `""`, so
            // `String(undefined)` printed nothing where Node prints
            // `undefined`.
            (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::PyStr)
                ) =>
            {
                // The PRESENT arm stringifies the inner value the way that type
                // stringifies on its own, so a `number | undefined` holding
                // `1e21` prints what `String(1e21)` does.
                let python = self.absent_spelling() == AbsentSpelling::None;
                let present = match self.mir.types.get(*inner) {
                    Some(Type::Int) if python => "value.to_string()".to_owned(),
                    Some(Type::Float) if python => format!(
                        "{fn_name}(value)",
                        fn_name = smelt_stdlib::runtime_symbols::number_text::PY_FLOAT_REPR_FN,
                    ),
                    Some(Type::Int) => format!(
                        "{fn_name}(value as f64)",
                        fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
                    ),
                    Some(Type::Float) => format!(
                        "{fn_name}(value)",
                        fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
                    ),
                    _ => "value.to_string()".to_owned(),
                };
                Ok(RustExpr::postfix_on(&operand_expr, format!(
                    "{operand_expr}.map_or_else(|| {:?}.to_owned(), |value| {present})",
                    self.absent_spelling().text(),
                )))
            }
            (smelt_hir::PrimitiveCastOp::ToString, Type::String | Type::PyStr, _) => {
                self.string_like_operand_expr(operand, "String")
            }
            (_, Type::Bool, _) => Ok(RustExpr::path("false")),
            (_, Type::Int, _) => Ok(RustExpr::path("0_i64")),
            (_, Type::Float, _) => Ok(RustExpr::path("0.0")),
            (_, Type::String, _) => Ok(RustExpr::atom("JsString::new()")),
            (_, Type::PyStr, _) => Ok(RustExpr::atom("String::new()")),
            (_, Type::Unknown | Type::Union(_) | Type::Never, _) => self.erase(operand),
            _ => self.default_value(dest_ty),
        }
    }

    /// Emit JavaScript numeric conversion for a Rust string expression: the
    /// shared `smelt_string_to_number` runtime helper (ECMA-262
    /// `StringToNumber`), the one string-to-number rule every emit site uses.
    fn js_number_from_string_expr(&self, operand_expr: &RustExpr) -> RustExpr {
        RustExpr::from(string_to_number_call(operand_expr, self.indexes_utf16()))
    }

    /// Emit JavaScript numeric conversion for an erased runtime value.
    fn js_number_from_unknown_expr(&self, operand_expr: &RustExpr) -> RustExpr {
        let string_conversion = self.js_number_from_string_expr(&RustExpr::path("value"));
        RustExpr::atom(self.carrier_format(format_args!(
            "match {operand_expr} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => {string_conversion}, SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null => 0.0, SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),")
        )))
    }
}

/// The call text that converts the string expression `text` with the
/// JavaScript rule, for an emit site that builds its code with `format!`.
///
/// `js_text` says whether `text` is a JavaScript string (`JsString`) or a Rust
/// `String`/`&str`.
pub(super) fn string_to_number_call(text: &str, js_text: bool) -> String {
    use smelt_stdlib::runtime_symbols::number_text::{JS_STRING_TO_NUMBER_FN, STRING_TO_NUMBER_FN};
    if js_text {
        format!("{JS_STRING_TO_NUMBER_FN}(&({text}))")
    } else {
        format!("{STRING_TO_NUMBER_FN}(&({text}))")
    }
}
