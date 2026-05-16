//! Strings Io emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a timestamp in milliseconds to an RFC 3339 timestamp string.
    pub(super) fn date_to_iso_string_text(
        &self,
        timestamp_ms: &Operand,
    ) -> Result<String, EmitError> {
        let timestamp_text = self.date_timestamp_text(timestamp_ms)?;
        Ok(format!(
            "chrono::DateTime::<chrono::Utc>::from_timestamp_millis({timestamp_text} as i64).map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(|| \"Invalid Date\".to_owned())"
        ))
    }

    /// Converts a DateArg-compatible operand to a numeric timestamp expression.
    ///
    /// Frontends model JavaScript `Date` values as timestamps where possible,
    /// but real libraries pass Date-compatible erased values through generic
    /// option bags and formatter APIs. Rust emission accepts those surfaces here
    /// and maps values that cannot produce a timestamp to `NaN`, matching the
    /// existing invalid-date sentinel path.
    fn date_timestamp_text(&self, timestamp_ms: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(timestamp_ms)?;
        let text = self.operand_text(timestamp_ms)?;
        self.date_timestamp_value_text(&text, ty)
    }

    /// Converts rendered DateArg-compatible Rust value text to timestamp text.
    fn date_timestamp_value_text(&self, value_text: &str, ty: TypeId) -> Result<String, EmitError> {
        match self.mir.types.get(ty) {
            Some(Type::Int | Type::Float) => Ok(value_text.to_owned()),
            Some(Type::Bool) => Ok(format!("if {value_text} {{ 1.0 }} else {{ 0.0 }}")),
            Some(Type::String) => Ok(format!("{value_text}.parse::<f64>().unwrap_or(f64::NAN)")),
            Some(Type::Optional(inner)) => {
                let inner_text = self.date_timestamp_value_text("value", *inner)?;
                Ok(format!(
                    "match {value_text} {{ Some(value) => {inner_text}, None => f64::NAN }}"
                ))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
            | Some(Type::Class { .. })
                if self.is_erased_class_type(ty)
                    || matches!(
                        self.mir.types.get(ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
                    ) =>
            {
                Ok(format!(
                    "match {value_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => f64::NAN }}"
                ))
            }
            _ => Ok("f64::NAN".to_owned()),
        }
    }

    /// Converts JavaScript Date constructor parts to a timestamp in milliseconds.
    pub(super) fn date_from_parts_text(&self, parts: &[Operand]) -> Result<String, EmitError> {
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
            values.push(self.operand_text(part)?);
        }
        while values.len() < 7 {
            values.push(if values.len() == 2 {
                "1.0".to_owned()
            } else {
                "0.0".to_owned()
            });
        }
        let [year, month, day, hour, minute, second, milli] = values.as_slice() else {
            return Err(EmitError::new(
                "Date constructor internal part count mismatch",
            ));
        };
        Ok(format!(
            "{{ let year = {year} as i32; let month0 = {month} as u32; let day = {day} as u32; let hour = {hour} as u32; let minute = {minute} as u32; let second = {second} as u32; let milli = {milli} as u32; chrono::NaiveDate::from_ymd_opt(year, month0 + 1, day).and_then(|date| date.and_hms_milli_opt(hour, minute, second, milli)).map(|dt| dt.and_utc().timestamp_millis()).unwrap_or(i64::MIN) }}",
        ))
    }

    /// Converts a JavaScript local date getter to Rust text.
    pub(super) fn date_get_part_text(
        &self,
        part: smelt_hir::DatePart,
        timestamp_ms: &Operand,
    ) -> Result<String, EmitError> {
        let timestamp_text = self.date_timestamp_text(timestamp_ms)?;
        let (trait_use, accessor) = match part {
            smelt_hir::DatePart::FullYear => ("chrono::Datelike as _", "date.year() as f64"),
            smelt_hir::DatePart::Month => ("chrono::Datelike as _", "date.month0() as f64"),
            smelt_hir::DatePart::Date => ("chrono::Datelike as _", "date.day() as f64"),
            smelt_hir::DatePart::Day => (
                "chrono::Datelike as _",
                "date.weekday().num_days_from_sunday() as f64",
            ),
            smelt_hir::DatePart::Hour => ("chrono::Timelike as _", "date.hour() as f64"),
            smelt_hir::DatePart::Minute => ("chrono::Timelike as _", "date.minute() as f64"),
            smelt_hir::DatePart::Second => ("chrono::Timelike as _", "date.second() as f64"),
            smelt_hir::DatePart::Millisecond => (
                "chrono::Timelike as _",
                "(date.nanosecond() / 1_000_000) as f64",
            ),
        };
        Ok(format!(
            "{{ use {trait_use}; chrono::DateTime::<chrono::Utc>::from_timestamp_millis({timestamp_text} as i64).map_or(f64::NAN, |date| {accessor}) }}"
        ))
    }

    /// Converts a JavaScript local date setter to a replacement timestamp.
    pub(super) fn date_set_part_text(
        &self,
        part: smelt_hir::DatePart,
        timestamp_ms: &Operand,
        values: &[Operand],
    ) -> Result<String, EmitError> {
        if values.is_empty() {
            return Err(EmitError::new("Date setter requires at least one value"));
        }
        let timestamp_text = self.date_timestamp_text(timestamp_ms)?;
        let value_texts = values
            .iter()
            .map(|value| self.date_timestamp_text(value))
            .collect::<Result<Vec<_>, _>>()?;
        let update = match part {
            smelt_hir::DatePart::FullYear => {
                let Some(year) = value_texts.first() else {
                    return Err(EmitError::new("Date.setFullYear requires a year value"));
                };
                let month = value_texts.get(1).map_or("date.month0()", String::as_str);
                let day = value_texts.get(2).map_or("date.day()", String::as_str);
                format!(
                    "date.with_year({year} as i32).and_then(|date| date.with_month0({month} as u32)).and_then(|date| date.with_day({day} as u32))"
                )
            }
            smelt_hir::DatePart::Month => {
                let Some(month) = value_texts.first() else {
                    return Err(EmitError::new("Date.setMonth requires a month value"));
                };
                let day = value_texts.get(1).map_or("date.day()", String::as_str);
                format!(
                    "date.with_month0({month} as u32).and_then(|date| date.with_day({day} as u32))"
                )
            }
            smelt_hir::DatePart::Date => {
                let Some(day) = value_texts.first() else {
                    return Err(EmitError::new("Date.setDate requires a day value"));
                };
                format!("date.with_day({day} as u32)")
            }
            smelt_hir::DatePart::Day => {
                return Err(EmitError::new("Date.setDay is not a JavaScript API"));
            }
            smelt_hir::DatePart::Hour => {
                let Some(hour) = value_texts.first() else {
                    return Err(EmitError::new("Date.setHours requires an hour value"));
                };
                let minute = value_texts.get(1).map_or("date.minute()", String::as_str);
                let second = value_texts.get(2).map_or("date.second()", String::as_str);
                let nano = value_texts.get(3).map_or_else(
                    || "date.nanosecond()".to_owned(),
                    |milli| format!("({milli} as u32) * 1_000_000"),
                );
                format!(
                    "date.with_hour({hour} as u32).and_then(|date| date.with_minute({minute} as u32)).and_then(|date| date.with_second({second} as u32)).and_then(|date| date.with_nanosecond({nano}))"
                )
            }
            smelt_hir::DatePart::Minute => {
                let Some(minute) = value_texts.first() else {
                    return Err(EmitError::new("Date.setMinutes requires a minute value"));
                };
                let second = value_texts.get(1).map_or("date.second()", String::as_str);
                let nano = value_texts.get(2).map_or_else(
                    || "date.nanosecond()".to_owned(),
                    |milli| format!("({milli} as u32) * 1_000_000"),
                );
                format!(
                    "date.with_minute({minute} as u32).and_then(|date| date.with_second({second} as u32)).and_then(|date| date.with_nanosecond({nano}))"
                )
            }
            smelt_hir::DatePart::Second => {
                let Some(second) = value_texts.first() else {
                    return Err(EmitError::new("Date.setSeconds requires a second value"));
                };
                let nano = value_texts.get(1).map_or_else(
                    || "date.nanosecond()".to_owned(),
                    |milli| format!("({milli} as u32) * 1_000_000"),
                );
                format!(
                    "date.with_second({second} as u32).and_then(|date| date.with_nanosecond({nano}))"
                )
            }
            smelt_hir::DatePart::Millisecond => {
                let Some(milli) = value_texts.first() else {
                    return Err(EmitError::new(
                        "Date.setMilliseconds requires a millisecond value",
                    ));
                };
                format!("date.with_nanosecond(({milli} as u32) * 1_000_000)")
            }
        };
        Ok(format!(
            "{{ use chrono::{{Datelike as _, Timelike as _}}; chrono::DateTime::<chrono::Utc>::from_timestamp_millis({timestamp_text} as i64).and_then(|date| {update}).map(|date| date.timestamp_millis()).unwrap_or(i64::MIN) }}"
        ))
    }

    /// Converts a parsed URL field access to Rust text using the `url` crate.
    /// Converts a parsed URL field access to Rust text using the `url` crate.
    pub(super) fn url_field_text(
        &self,
        field: smelt_hir::UrlField,
        url: &Operand,
    ) -> Result<String, EmitError> {
        self.require_string_operands(&[url], "URL field access")?;
        let url_text = self.operand_text(url)?;
        let parsed = format!("url::Url::parse(&{url_text}).expect(\"URL parse failed\")");
        Ok(match field {
            smelt_hir::UrlField::Href => format!("{parsed}.to_string()"),
            smelt_hir::UrlField::Protocol => format!("format!(\"{{}}:\", {parsed}.scheme())"),
            smelt_hir::UrlField::Host => format!(
                "{{ let url = {parsed}; url.host_str().map(|host| match url.port() {{ Some(port) => format!(\"{{}}:{{}}\", host, port), None => host.to_owned() }}).unwrap_or_default() }}"
            ),
            smelt_hir::UrlField::Hostname => {
                format!("{parsed}.host_str().unwrap_or_default().to_owned()")
            }
            smelt_hir::UrlField::Pathname => format!("{parsed}.path().to_owned()"),
            smelt_hir::UrlField::Search => {
                format!("{parsed}.query().map(|query| format!(\"?{{query}}\")).unwrap_or_default()")
            }
        })
    }

    /// Converts a text-file read to Rust text.
    /// Converts a text-file read to Rust text.
    pub(super) fn file_read_text(&self, path: &Operand) -> Result<String, EmitError> {
        self.require_string_operands(&[path], "file read")?;
        Ok(format!(
            "std::fs::read_to_string(&{}).expect(\"file read failed\")",
            self.operand_text(path)?
        ))
    }

    /// Converts a text-file write to Rust text, returning bytes written.
    /// Converts a text-file write to Rust text, returning bytes written.
    pub(super) fn file_write_text(
        &self,
        path: &Operand,
        text: &Operand,
    ) -> Result<String, EmitError> {
        self.require_string_operands(&[path, text], "file write")?;
        let path_text = self.operand_text(path)?;
        let text_text = self.operand_text(text)?;
        Ok(format!(
            "{{ let contents = {text_text}; std::fs::write(&{path_text}, &contents).expect(\"file write failed\"); contents.len() as i64 }}"
        ))
    }

    // Checks that every operand has string type.
}
