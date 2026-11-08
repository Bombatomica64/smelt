//! Strings Io emission helpers.

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Whether `ty` is the concrete `Date` class (`SmeltDate`, see
    /// `date_prelude`), as opposed to an erased or user-declared class.
    pub(super) fn is_concrete_date_type(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::Class { name, .. })
            if !self.is_erased_class_type(ty)
                && self.stdlib_class_of_symbol(*name).ok().flatten()
                    == Some(smelt_stdlib::StdlibClass::Date))
    }

    /// The `SmeltDate` handle behind a value of type `ty`, as Rust text: the
    /// value itself for a concrete `Date`, and the `DateLike` view for a type
    /// parameter bounded by `Date` that is a real generic here
    /// (`D: DateLike`, see `date_prelude`). `None` for any other type, which
    /// keeps the erased date paths.
    pub(super) fn date_handle_expr(&self, value_text: &RustExpr, ty: TypeId) -> Option<RustExpr> {
        if self.is_concrete_date_type(ty) {
            return Some(RustExpr::atom(format!("({value_text})", value_text = value_text.delimited())));
        }
        let Some(Type::TypeParam { name }) = self.mir.types.get(ty) else {
            return None;
        };
        (self.current_function_type_params().contains(name)
            && crate::bound_traits::type_param_is_date_bounded(self.mir, self.function, *name))
        .then(|| format!("({value_text}).smelt_date()", value_text = value_text.delimited())).map(RustExpr::from)
    }

    /// Coerce an `i64` JavaScript timestamp expression into the destination type.
    pub(super) fn date_timestamp_result_expr(
        &self,
        text: &RustExpr,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // A fresh `Date`: a new handle on the time value (`TimeClip` folds the
        // `i64::MIN` invalid sentinel and every non-finite value to `NaN`).
        if self.is_concrete_date_type(dest_ty) {
            return Ok(RustExpr::atom(format!("SmeltDate::new(({text}) as f64)", text = text.delimited())));
        }
        match self.mir.types.get(dest_ty) {
            Some(Type::Float) => Ok(RustExpr::atom(format!(
                "{{ let smelt_timestamp_ms = ({text}) as f64; if !smelt_timestamp_ms.is_finite() || smelt_timestamp_ms == i64::MIN as f64 {{ f64::NAN }} else {{ smelt_timestamp_ms }} }}", text = text.delimited()
            ))),
            // An erased destination holds the Date as its marker record.
            _ if self.has_dynamic_shape(dest_ty) => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{{ let smelt_timestamp_ms = ({text}) as f64; let smelt_timestamp_ms = if !smelt_timestamp_ms.is_finite() || smelt_timestamp_ms == i64::MIN as f64 {{ f64::NAN }} else {{ smelt_timestamp_ms }}; SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"{SMELT_DATE}\".to_owned(), SmeltUnknown::Number(smelt_timestamp_ms))]))) }}", text = text.delimited()
            )))),
            Some(Type::Optional(inner)) => {
                let inner_text = self.date_timestamp_result_expr(text, *inner)?;
                Ok(RustExpr::chain(format!("Some({inner_text})", inner_text = inner_text.delimited())))
            }
            _ => Ok(text.to_owned()),
        }
    }

    /// A setter's replacement timestamp `text`, stored back into the receiver.
    ///
    /// A concrete `Date` is a shared handle, so `d.setDate(1)` writes the new
    /// time value into the SAME cell every alias reads, and answers that date
    /// (MIR re-binds the receiver to the setter's result). Any other receiver or
    /// destination keeps the erased path, which rebuilds the marker record and
    /// copies its metadata (`__smelt_timezone`, subclass fields) across.
    pub(super) fn date_setter_result_expr(
        &self,
        text: &RustExpr,
        dest_ty: TypeId,
        receiver: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let receiver_text = self.operand_expr(receiver)?;
        if let Some(handle) = self.date_handle_expr(&receiver_text, receiver_ty)
            && self.date_handle_expr(&RustExpr::path("_"), dest_ty).is_some()
        {
            // The setter answers its receiver: the same date for a concrete
            // `Date`, the same `D` for a `D: DateLike` receiver.
            let answer = if receiver_ty == dest_ty {
                receiver_text.clone()
            } else {
                handle.clone()
            };
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_time_ms = ({text}) as f64; {handle}.set_time(smelt_time_ms); {answer} }}", text = text.delimited(), handle = handle.tight(), answer = answer.delimited()
            )));
        }
        self.date_timestamp_result_preserving_receiver_expr(text, dest_ty, receiver)
    }

    /// Wrap a replacement timestamp while preserving metadata from an erased Date receiver.
    ///
    /// Date subclasses and context-backed dates carry behavior through fields on
    /// their erased object representation. JavaScript setters mutate that same
    /// Date kind, so rebuilding only `__smelt_date` would incorrectly discard
    /// timezone and subclass metadata.
    pub(super) fn date_timestamp_result_preserving_receiver_expr(
        &self,
        text: &RustExpr,
        dest_ty: TypeId,
        receiver: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let result_text = self.date_timestamp_result_expr(text, dest_ty)?;
        let receiver_ty = self.operand_ty(receiver)?;
        let receiver_is_erased = self.is_erased_class_type(receiver_ty)
            || matches!(
                self.mir.types.get(receiver_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            );
        if !receiver_is_erased
            || !(self.is_erased_class_type(dest_ty)
                || matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ))
        {
            return Ok(result_text);
        }
        let receiver_text = self.operand_expr(receiver)?;
        Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "{{ let smelt_date_result = {result_text}; if let (SmeltUnknown::Object(smelt_result), SmeltUnknown::Object(receiver)) = (&smelt_date_result, {receiver_text}) {{ for (smelt_key, smelt_value) in receiver.iter() {{ if smelt_key != \"{SMELT_DATE}\" {{ smelt_result.insert(smelt_key, smelt_value); }} }} }} smelt_date_result }}", result_text = result_text.delimited(), receiver_text = receiver_text.delimited()
        ))))
    }

    /// Converts a timestamp in milliseconds to an RFC 3339 timestamp string.
    pub(super) fn date_to_iso_string_expr(
        &self,
        timestamp_ms: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(timestamp_ms)?;
        let value_text = self.operand_expr(timestamp_ms)?;
        if let Some(handle) = self.date_handle_expr(&value_text, ty) {
            return Ok(handle.postfix(".to_iso_string()"));
        }
        if self.is_erased_class_type(ty)
            || matches!(
                self.mir.types.get(ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
            )
        {
            return Ok(RustExpr::atom(format!("({value_text}).to_iso_string()", value_text = value_text.delimited())));
        }
        let timestamp_text = self.date_timestamp_expr(timestamp_ms)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_timestamp_ms = ({timestamp_text}) as f64; if smelt_timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).map(|smelt_date| smelt_date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(|| \"Invalid Date\".to_owned()) }} else {{ \"Invalid Date\".to_owned() }} }}", timestamp_text = timestamp_text.delimited()
        )))
    }

    /// Converts a timestamp in milliseconds to JavaScript Date string output.
    pub(super) fn date_to_string_expr(&self, timestamp_ms: &Operand) -> Result<RustExpr, EmitError> {
        let timestamp_text = self.date_timestamp_expr(timestamp_ms)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_timestamp_ms = ({timestamp_text}) as f64; if smelt_timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).map(|smelt_date| smelt_date.to_rfc2822()).unwrap_or_else(|| \"Invalid Date\".to_owned()) }} else {{ \"Invalid Date\".to_owned() }} }}", timestamp_text = timestamp_text.delimited()
        )))
    }

    /// Converts a Date timestamp to `Date.prototype.toUTCString()` text: the
    /// RFC 7231 IMF-fixdate (`Sun, 24 Dec 2000 10:30:59 GMT`), which is what
    /// HTTP `Expires`/`Date` headers carry. An invalid date reads
    /// `"Invalid Date"`, as JavaScript's does.
    pub(super) fn date_to_utc_string_expr(&self, timestamp_ms: &Operand) -> Result<RustExpr, EmitError> {
        let timestamp_text = self.date_timestamp_expr(timestamp_ms)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_timestamp_ms = ({timestamp_text}) as f64; if smelt_timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).map(|smelt_date| smelt_date.format(\"%a, %d %b %Y %H:%M:%S GMT\").to_string()).unwrap_or_else(|| \"Invalid Date\".to_owned()) }} else {{ \"Invalid Date\".to_owned() }} }}", timestamp_text = timestamp_text.delimited()
        )))
    }

    /// `JSON.stringify(value, replacer, space)`'s indentation: the compact text
    /// re-laid out by `smelt_json_indent` with the indent `space` names.
    ///
    /// The indent string is computed here from the operand's type: a number is
    /// that many spaces (clamped to 10, below 1 is none), a string its first
    /// 10 characters, anything else (`null`, `undefined`, an erased value that
    /// is neither) no indentation, which is the compact text unchanged.
    pub(super) fn json_indent_expr(&self, text: &Operand, space: &Operand) -> Result<RustExpr, EmitError> {
        let text_text = self.operand_expr(text)?;
        let space_ty = self.operand_ty(space)?;
        let space_text = self.operand_expr(space)?;
        let indent = match self.mir.types.get(space_ty) {
            Some(Type::Float | Type::Int) => format!(
                "{{ let smelt_width = ({space_text}) as f64; if smelt_width >= 1.0 {{ \" \".repeat(smelt_width.min(10.0) as usize) }} else {{ String::new() }} }}", space_text = space_text.delimited()
            ),
            Some(Type::String) => format!("({space_text}).slice(0, 10).to_string_lossy()", space_text = space_text.delimited()),
            Some(Type::PyStr) => format!("({space_text}).chars().take(10).collect::<String>()", space_text = space_text.delimited()),
            Some(Type::Optional(inner)) if matches!(self.mir.types.get(*inner), Some(Type::Float | Type::Int)) => format!(
                "({space_text}).map_or(String::new(), |smelt_width| {{ let smelt_width = smelt_width as f64; if smelt_width >= 1.0 {{ \" \".repeat(smelt_width.min(10.0) as usize) }} else {{ String::new() }} }})", space_text = space_text.delimited()
            ),
            Some(Type::Optional(inner)) if matches!(self.mir.types.get(*inner), Some(Type::String)) => {
                format!("({space_text}).map_or(String::new(), |smelt_space| smelt_space.slice(0, 10).to_string_lossy())", space_text = space_text.delimited())
            }
            Some(Type::Optional(inner)) if matches!(self.mir.types.get(*inner), Some(Type::PyStr)) => {
                format!("({space_text}).map_or(String::new(), |smelt_space| smelt_space.chars().take(10).collect::<String>())", space_text = space_text.delimited())
            }
            Some(Type::Unknown | Type::Union(_)) => self.carrier_format(self.carrier()?, format_args!(
                "match ({}) {{ SmeltUnknown::Number(smelt_width) if smelt_width >= 1.0 => \" \".repeat(smelt_width.min(10.0) as usize), SmeltUnknown::String(smelt_space) => smelt_space.slice(0, 10).to_string_lossy(), _ => String::new() }}",
                self.erase(space)?.delimited()
            )),
            _ => "String::new()".to_owned(),
        };
        Ok(RustExpr::chain(self.text_from_string(&format!("smelt_json_indent(&{text_text}, &{indent})", text_text = text_text.tight()))))
    }

    /// Converts a DateArg-compatible operand to a numeric timestamp expression.
    ///
    /// Frontends model JavaScript `Date` values as timestamps where possible,
    /// but real libraries pass Date-compatible erased values through generic
    /// option bags and formatter APIs. Rust emission accepts those surfaces here
    /// and maps values that cannot produce a timestamp to `NaN`, matching the
    /// existing invalid-date sentinel path.
    pub(super) fn date_timestamp_expr(&self, timestamp_ms: &Operand) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(timestamp_ms)?;
        let text = self.operand_expr(timestamp_ms)?;
        self.date_timestamp_value(&text, ty)
    }

    /// Converts rendered DateArg-compatible Rust value text to timestamp text.
    fn date_timestamp_value(&self, value_text: &RustExpr, ty: TypeId) -> Result<RustExpr, EmitError> {
        if let Some(handle) = self.date_handle_expr(value_text, ty) {
            return Ok(handle.postfix(".get_time()"));
        }
        // A concrete union (`Date | number | string` once `Date` is concrete)
        // is a generated enum; the erased arm below reads its erased value.
        if self.concrete_union_members(ty).is_some() {
            let erased = self.erase_rendered(value_text, ty)?;
            let unknown_ty = self.type_id(Type::Unknown)?;
            return self.date_timestamp_value(&erased, unknown_ty);
        }
        match self.mir.types.get(ty) {
            Some(Type::Int | Type::Float) => Ok(value_text.to_owned()),
            Some(Type::Bool) => Ok(RustExpr::loose(format!("if {value_text} {{ 1.0 }} else {{ 0.0 }}", value_text = value_text.delimited()))),
            Some(Type::String | Type::PyStr) => Ok(RustExpr::atom(format!(
                "{{ let smelt_date_text = {date_text}; chrono::DateTime::parse_from_rfc3339(&smelt_date_text).or_else(|_| chrono::DateTime::parse_from_str(&smelt_date_text, \"%a %b %d %Y %H:%M:%S GMT%z\")).map(|smelt_date| smelt_date.timestamp_millis() as f64).or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_date_text, \"%Y/%m/%d\").or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_date_text, \"%Y-%m-%d\")).map(|smelt_date| smelt_date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64)).unwrap_or_else(|_| smelt_date_text.parse::<f64>().unwrap_or(f64::NAN)) }}", date_text = self.host_utf8_text(value_text.clone()).delimited()))),
            Some(Type::Optional(inner)) => {
                let inner_text = self.date_timestamp_value(&RustExpr::path("smelt_value"), *inner)?;
                Ok(RustExpr::atom(format!(
                    "match {value_text} {{ Some(smelt_value) => {inner_text}, None => f64::NAN }}", value_text = value_text.delimited(), inner_text = inner_text.delimited()
                )))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
            | Some(Type::Class { .. })
                if self.is_erased_class_type(ty)
                    || matches!(
                        self.mir.types.get(ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
                    ) =>
            {
                Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "match {value_text} {{ SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }}, SmeltUnknown::String(smelt_value) => {{ let smelt_value = smelt_value.to_string_lossy(); chrono::DateTime::parse_from_rfc3339(&smelt_value).or_else(|_| chrono::DateTime::parse_from_str(&smelt_value, \"%a %b %d %Y %H:%M:%S GMT%z\")).map(|smelt_date| smelt_date.timestamp_millis() as f64).or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_value, \"%Y/%m/%d\").or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_value, \"%Y-%m-%d\")).map(|smelt_date| smelt_date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64)).unwrap_or_else(|_| smelt_value.parse::<f64>().unwrap_or(f64::NAN)) }}, SmeltUnknown::Bool(smelt_value) => if smelt_value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),"), value_text = value_text.delimited()
                ))))
            }
            _ => Ok(RustExpr::atom("f64::NAN")),
        }
    }

    /// Converts JavaScript Date constructor parts to a timestamp in milliseconds.
    pub(super) fn date_from_parts_expr(&self, parts: &[Operand]) -> Result<RustExpr, EmitError> {
        if parts.is_empty() || parts.len() > 7 {
            return Err(EmitError::new(
                "Date constructor expects one to seven arguments",
            ));
        }
        for part in parts {
            if !matches!(
                self.mir.types.get(self.operand_ty(part)?),
                Some(Type::Int | Type::Float)
            ) {
                return Err(EmitError::new("Date constructor parts must be numeric"));
            }
        }
        let mut values = Vec::with_capacity(7);
        for part in parts {
            values.push(self.operand_expr(part)?);
        }
        while values.len() < 7 {
            values.push(if values.len() == 2 {
                RustExpr::path("1.0")
            } else {
                RustExpr::path("0.0")
            });
        }
        let [year, month, day, hour, minute, second, milli] = values.as_slice() else {
            return Err(EmitError::new(
                "Date constructor internal part count mismatch",
            ));
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_year_value = ({year}) as f64; let smelt_month_value = ({month}) as f64; let smelt_day_value = ({day}) as f64; let smelt_hour_value = ({hour}) as f64; let smelt_minute_value = ({minute}) as f64; let smelt_second_value = ({second}) as f64; let smelt_milli_value = ({milli}) as f64; if [smelt_year_value, smelt_month_value, smelt_day_value, smelt_hour_value, smelt_minute_value, smelt_second_value, smelt_milli_value].into_iter().all(f64::is_finite) {{ let smelt_year = smelt_year_value as i32; let smelt_month0 = smelt_month_value as u32; let smelt_day = smelt_day_value as u32; let smelt_hour = smelt_hour_value as u32; let smelt_minute = smelt_minute_value as u32; let smelt_second = smelt_second_value as u32; let smelt_milli = smelt_milli_value as u32; chrono::NaiveDate::from_ymd_opt(smelt_year, smelt_month0 + 1, smelt_day).and_then(|smelt_date| smelt_date.and_hms_milli_opt(smelt_hour, smelt_minute, smelt_second, smelt_milli)).map(|smelt_dt| smelt_dt.and_utc().timestamp_millis()).unwrap_or(i64::MIN) }} else {{ i64::MIN }} }}", year = year.delimited(), month = month.delimited(), day = day.delimited(), hour = hour.delimited(), minute = minute.delimited(), second = second.delimited(), milli = milli.delimited(),
        )))
    }

    /// Converts a JavaScript local date getter to Rust text.
    pub(super) fn date_get_part_expr(
        &self,
        part: smelt_hir::DatePart,
        timestamp_ms: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let timestamp_text = self.date_timestamp_expr(timestamp_ms)?;
        let (trait_use, accessor) = match part {
            smelt_hir::DatePart::FullYear => ("chrono::Datelike as _", "smelt_date.year() as f64"),
            smelt_hir::DatePart::Month => ("chrono::Datelike as _", "smelt_date.month0() as f64"),
            smelt_hir::DatePart::Date => ("chrono::Datelike as _", "smelt_date.day() as f64"),
            smelt_hir::DatePart::Day => (
                "chrono::Datelike as _",
                "smelt_date.weekday().num_days_from_sunday() as f64",
            ),
            smelt_hir::DatePart::Hour => ("chrono::Timelike as _", "smelt_date.hour() as f64"),
            smelt_hir::DatePart::Minute => ("chrono::Timelike as _", "smelt_date.minute() as f64"),
            smelt_hir::DatePart::Second => ("chrono::Timelike as _", "smelt_date.second() as f64"),
            smelt_hir::DatePart::Millisecond => (
                "chrono::Timelike as _",
                "(smelt_date.nanosecond() / 1_000_000) as f64",
            ),
        };
        Ok(RustExpr::atom(format!(
            "{{ use {trait_use}; let smelt_timestamp_ms = ({timestamp_text}) as f64; if smelt_timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).map_or(f64::NAN, |smelt_date| {accessor}) }} else {{ f64::NAN }} }}", timestamp_text = timestamp_text.delimited()
        )))
    }

    /// Converts a JavaScript local date setter to a replacement timestamp.
    pub(super) fn date_set_part_expr(
        &self,
        part: smelt_hir::DatePart,
        timestamp_ms: &Operand,
        values: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        if values.is_empty() {
            return Err(EmitError::new("Date setter requires at least one value"));
        }
        let timestamp_text = self.date_timestamp_expr(timestamp_ms)?;
        let value_texts = values
            .iter()
            .map(|value| self.date_timestamp_expr(value))
            .collect::<Result<Vec<_>, _>>()?;
        let values_are_finite = value_texts
            .iter()
            .map(|value| format!("(({value}) as f64).is_finite()", value = value.delimited()))
            .collect::<Vec<_>>()
            .join(" && ");
        let update = match part {
            smelt_hir::DatePart::FullYear => {
                let Some(year) = value_texts.first() else {
                    return Err(EmitError::new("Date.setFullYear requires a year value"));
                };
                let month = value_texts.get(1).map_or("smelt_date.month0()", RustExpr::as_str);
                let day = value_texts.get(2).map_or("smelt_date.day()", RustExpr::as_str);
                format!(
                    "{{ let smelt_month_index = {month} as i32; let smelt_normalized_year = ({year} as i32) + smelt_month_index.div_euclid(12); let smelt_normalized_month0 = smelt_month_index.rem_euclid(12) as u32; chrono::NaiveDate::from_ymd_opt(smelt_normalized_year, smelt_normalized_month0 + 1, 1).and_then(|smelt_base| smelt_base.and_hms_nano_opt(smelt_date.hour(), smelt_date.minute(), smelt_date.second(), smelt_date.nanosecond())).map(|smelt_base| smelt_base + chrono::Duration::days(({day} as i64) - 1)).map(|smelt_date| smelt_date.and_utc()) }}", year = year.as_cast_operand()
                )
            }
            smelt_hir::DatePart::Month => {
                let Some(month) = value_texts.first() else {
                    return Err(EmitError::new("Date.setMonth requires a month value"));
                };
                let day = value_texts.get(1).map_or("smelt_date.day()", RustExpr::as_str);
                format!(
                    "{{ let smelt_month_index = {month} as i32; let smelt_normalized_year = smelt_date.year() + smelt_month_index.div_euclid(12); let smelt_normalized_month0 = smelt_month_index.rem_euclid(12) as u32; chrono::NaiveDate::from_ymd_opt(smelt_normalized_year, smelt_normalized_month0 + 1, 1).and_then(|smelt_base| smelt_base.and_hms_nano_opt(smelt_date.hour(), smelt_date.minute(), smelt_date.second(), smelt_date.nanosecond())).map(|smelt_base| smelt_base + chrono::Duration::days(({day} as i64) - 1)).map(|smelt_date| smelt_date.and_utc()) }}", month = month.as_cast_operand()
                )
            }
            smelt_hir::DatePart::Date => {
                let Some(day) = value_texts.first() else {
                    return Err(EmitError::new("Date.setDate requires a day value"));
                };
                format!(
                    "chrono::NaiveDate::from_ymd_opt(smelt_date.year(), smelt_date.month(), 1).and_then(|smelt_base| smelt_base.and_hms_nano_opt(smelt_date.hour(), smelt_date.minute(), smelt_date.second(), smelt_date.nanosecond())).map(|smelt_base| smelt_base + chrono::Duration::days(({day} as i64) - 1)).map(|smelt_date| smelt_date.and_utc())", day = day.as_cast_operand()
                )
            }
            smelt_hir::DatePart::Day => {
                return Err(EmitError::new("Date.setDay is not a JavaScript API"));
            }
            smelt_hir::DatePart::Hour => {
                let Some(hour) = value_texts.first() else {
                    return Err(EmitError::new("Date.setHours requires an hour value"));
                };
                let minute = value_texts.get(1).map_or("smelt_date.minute()", RustExpr::as_str);
                let second = value_texts.get(2).map_or("smelt_date.second()", RustExpr::as_str);
                let milli = value_texts
                    .get(3)
                    .map_or("smelt_date.timestamp_subsec_millis() as f64", RustExpr::as_str);
                format!(
                    "chrono::NaiveDate::from_ymd_opt(smelt_date.year(), smelt_date.month(), smelt_date.day()).and_then(|smelt_base| smelt_base.and_hms_milli_opt(0, 0, 0, 0)).map(|smelt_base| smelt_base + chrono::Duration::milliseconds((({hour} as i64) * 3_600_000) + (({minute} as i64) * 60_000) + (({second} as i64) * 1_000) + ({milli} as i64))).map(|smelt_date| smelt_date.and_utc())", hour = hour.as_cast_operand()
                )
            }
            smelt_hir::DatePart::Minute => {
                let Some(minute) = value_texts.first() else {
                    return Err(EmitError::new("Date.setMinutes requires a minute value"));
                };
                let second = value_texts.get(1).map_or("smelt_date.second()", RustExpr::as_str);
                let milli = value_texts
                    .get(2)
                    .map_or("smelt_date.timestamp_subsec_millis() as f64", RustExpr::as_str);
                format!(
                    "chrono::NaiveDate::from_ymd_opt(smelt_date.year(), smelt_date.month(), smelt_date.day()).and_then(|smelt_base| smelt_base.and_hms_milli_opt(smelt_date.hour(), 0, 0, 0)).map(|smelt_base| smelt_base + chrono::Duration::milliseconds((({minute} as i64) * 60_000) + (({second} as i64) * 1_000) + ({milli} as i64))).map(|smelt_date| smelt_date.and_utc())", minute = minute.as_cast_operand()
                )
            }
            smelt_hir::DatePart::Second => {
                let Some(second) = value_texts.first() else {
                    return Err(EmitError::new("Date.setSeconds requires a second value"));
                };
                let milli = value_texts
                    .get(1)
                    .map_or("smelt_date.timestamp_subsec_millis() as f64", RustExpr::as_str);
                format!(
                    "chrono::NaiveDate::from_ymd_opt(smelt_date.year(), smelt_date.month(), smelt_date.day()).and_then(|smelt_base| smelt_base.and_hms_milli_opt(smelt_date.hour(), smelt_date.minute(), 0, 0)).map(|smelt_base| smelt_base + chrono::Duration::milliseconds((({second} as i64) * 1_000) + ({milli} as i64))).map(|smelt_date| smelt_date.and_utc())", second = second.as_cast_operand()
                )
            }
            smelt_hir::DatePart::Millisecond => {
                let Some(milli) = value_texts.first() else {
                    return Err(EmitError::new(
                        "Date.setMilliseconds requires a millisecond value",
                    ));
                };
                format!(
                    "chrono::NaiveDate::from_ymd_opt(smelt_date.year(), smelt_date.month(), smelt_date.day()).and_then(|smelt_base| smelt_base.and_hms_milli_opt(smelt_date.hour(), smelt_date.minute(), smelt_date.second(), 0)).map(|smelt_base| smelt_base + chrono::Duration::milliseconds({milli} as i64)).map(|smelt_date| smelt_date.and_utc())", milli = milli.as_cast_operand()
                )
            }
        };
        Ok(RustExpr::atom(format!(
            "{{ use chrono::{{Datelike as _, Timelike as _}}; let smelt_timestamp_ms = ({timestamp_text}) as f64; if smelt_timestamp_ms.is_finite() && {values_are_finite} {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).and_then(|smelt_date| {update}).map(|smelt_date| smelt_date.timestamp_millis()).unwrap_or(i64::MIN) }} else {{ i64::MIN }} }}", timestamp_text = timestamp_text.delimited()
        )))
    }

    /// Converts a parsed URL field access to Rust text using the `url` crate.
    pub(super) fn url_field_expr(
        &self,
        field: smelt_hir::UrlField,
        url: &Operand,
    ) -> Result<RustExpr, EmitError> {
        self.require_string_operands(&[url], "URL field access")?;
        let url_text = self.operand_expr(url)?;
        let parsed = format!("url::Url::parse(&{url_text}).expect(\"URL parse failed\")", url_text = url_text.tight());
        // `url::quirks` is the WHATWG accessor set, so a string-held URL reads
        // each component exactly as a `URL` value's getter does.
        let getter = match field {
            smelt_hir::UrlField::Href => "href",
            smelt_hir::UrlField::Protocol => "protocol",
            smelt_hir::UrlField::Username => "username",
            smelt_hir::UrlField::Password => "password",
            smelt_hir::UrlField::Host => "host",
            smelt_hir::UrlField::Hostname => "hostname",
            smelt_hir::UrlField::Port => "port",
            smelt_hir::UrlField::Pathname => "pathname",
            smelt_hir::UrlField::Search => "search",
            smelt_hir::UrlField::Hash => "hash",
            smelt_hir::UrlField::Origin => {
                return Ok(RustExpr::atom(format!("url::quirks::origin(&{parsed})")));
            }
        };
        Ok(RustExpr::opaque(format!("url::quirks::{getter}(&{parsed}).to_owned()")))
    }

    /// Converts a text-file read to Rust text.
    pub(super) fn file_read_expr(&self, path: &Operand) -> Result<RustExpr, EmitError> {
        self.require_string_operands(&[path], "file read")?;
        Ok(RustExpr::atom(format!(
            "std::fs::read_to_string(&{}).expect(\"file read failed\")",
            self.operand_expr(path)?.tight()
        )))
    }

    /// Converts a text-file write to Rust text, returning bytes written.
    pub(super) fn file_write_expr(
        &self,
        path: &Operand,
        text: &Operand,
    ) -> Result<RustExpr, EmitError> {
        self.require_string_operands(&[path, text], "file write")?;
        let path_text = self.operand_expr(path)?;
        let text_text = self.operand_expr(text)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_contents = {text_text}; std::fs::write(&{path_text}, &smelt_contents).expect(\"file write failed\"); smelt_contents.len() as i64 }}", text_text = text_text.delimited(), path_text = path_text.tight()
        )))
    }

}
