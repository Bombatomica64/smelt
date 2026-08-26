//! Strings Io emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Coerce an `i64` JavaScript timestamp expression into the destination type.
    pub(super) fn date_timestamp_result_text(
        &self,
        text: &str,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(dest_ty) {
            Some(Type::Float) => Ok(format!(
                "{{ let timestamp_ms = ({text}) as f64; if !timestamp_ms.is_finite() || timestamp_ms == i64::MIN as f64 {{ f64::NAN }} else {{ timestamp_ms }} }}"
            )),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => Ok(format!(
                "{{ let timestamp_ms = ({text}) as f64; let timestamp_ms = if !timestamp_ms.is_finite() || timestamp_ms == i64::MIN as f64 {{ f64::NAN }} else {{ timestamp_ms }}; SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_date\".to_owned(), SmeltUnknown::Number(timestamp_ms))]))) }}"
            )),
            Some(Type::Optional(inner)) => {
                let inner_text = self.date_timestamp_result_text(text, *inner)?;
                Ok(format!("Some({inner_text})"))
            }
            _ if self.is_erased_class_type(dest_ty) => Ok(format!(
                "{{ let timestamp_ms = ({text}) as f64; let timestamp_ms = if !timestamp_ms.is_finite() || timestamp_ms == i64::MIN as f64 {{ f64::NAN }} else {{ timestamp_ms }}; SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_date\".to_owned(), SmeltUnknown::Number(timestamp_ms))]))) }}"
            )),
            _ => Ok(text.to_owned()),
        }
    }

    /// Wrap a replacement timestamp while preserving metadata from an erased Date receiver.
    ///
    /// Date subclasses and context-backed dates carry behavior through fields on
    /// their erased object representation. JavaScript setters mutate that same
    /// Date kind, so rebuilding only `__smelt_date` would incorrectly discard
    /// timezone and subclass metadata.
    pub(super) fn date_timestamp_result_preserving_receiver_text(
        &self,
        text: &str,
        dest_ty: TypeId,
        receiver: &Operand,
    ) -> Result<String, EmitError> {
        let result_text = self.date_timestamp_result_text(text, dest_ty)?;
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
        let receiver_text = self.operand_text(receiver)?;
        Ok(format!(
            "{{ let smelt_date_result = {result_text}; if let (SmeltUnknown::Object(result), SmeltUnknown::Object(receiver)) = (&smelt_date_result, {receiver_text}) {{ for (key, value) in receiver.iter() {{ if key != \"__smelt_date\" {{ result.insert(key, value); }} }} }} smelt_date_result }}"
        ))
    }

    /// Converts a timestamp in milliseconds to an RFC 3339 timestamp string.
    pub(super) fn date_to_iso_string_text(
        &self,
        timestamp_ms: &Operand,
    ) -> Result<String, EmitError> {
        let ty = self.operand_ty(timestamp_ms)?;
        let value_text = self.operand_text(timestamp_ms)?;
        if self.is_erased_class_type(ty)
            || matches!(
                self.mir.types.get(ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
            )
        {
            return Ok(format!("({value_text}).to_iso_string()"));
        }
        let timestamp_text = self.date_timestamp_text(timestamp_ms)?;
        Ok(format!(
            "{{ let timestamp_ms = ({timestamp_text}) as f64; if timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(|| \"Invalid Date\".to_owned()) }} else {{ \"Invalid Date\".to_owned() }} }}"
        ))
    }

    /// Converts a timestamp in milliseconds to JavaScript Date string output.
    pub(super) fn date_to_string_text(&self, timestamp_ms: &Operand) -> Result<String, EmitError> {
        let timestamp_text = self.date_timestamp_text(timestamp_ms)?;
        Ok(format!(
            "{{ let timestamp_ms = ({timestamp_text}) as f64; if timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.to_rfc2822()).unwrap_or_else(|| \"Invalid Date\".to_owned()) }} else {{ \"Invalid Date\".to_owned() }} }}"
        ))
    }

    /// Converts a DateArg-compatible operand to a numeric timestamp expression.
    ///
    /// Frontends model JavaScript `Date` values as timestamps where possible,
    /// but real libraries pass Date-compatible erased values through generic
    /// option bags and formatter APIs. Rust emission accepts those surfaces here
    /// and maps values that cannot produce a timestamp to `NaN`, matching the
    /// existing invalid-date sentinel path.
    pub(super) fn date_timestamp_text(&self, timestamp_ms: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(timestamp_ms)?;
        let text = self.operand_text(timestamp_ms)?;
        self.date_timestamp_value_text(&text, ty)
    }

    /// Converts rendered DateArg-compatible Rust value text to timestamp text.
    fn date_timestamp_value_text(&self, value_text: &str, ty: TypeId) -> Result<String, EmitError> {
        match self.mir.types.get(ty) {
            Some(Type::Int | Type::Float) => Ok(value_text.to_owned()),
            Some(Type::Bool) => Ok(format!("if {value_text} {{ 1.0 }} else {{ 0.0 }}")),
            Some(Type::String) => Ok(format!(
                "chrono::DateTime::parse_from_rfc3339(&{value_text}).or_else(|_| chrono::DateTime::parse_from_str(&{value_text}, \"%a %b %d %Y %H:%M:%S GMT%z\")).map(|date| date.timestamp_millis() as f64).or_else(|_| chrono::NaiveDate::parse_from_str(&{value_text}, \"%Y/%m/%d\").or_else(|_| chrono::NaiveDate::parse_from_str(&{value_text}, \"%Y-%m-%d\")).map(|date| date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64)).unwrap_or_else(|_| {value_text}.parse::<f64>().unwrap_or(f64::NAN))"
            )),
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
                    "match {value_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => chrono::DateTime::parse_from_rfc3339(&value).or_else(|_| chrono::DateTime::parse_from_str(&value, \"%a %b %d %Y %H:%M:%S GMT%z\")).map(|date| date.timestamp_millis() as f64).or_else(|_| chrono::NaiveDate::parse_from_str(&value, \"%Y/%m/%d\").or_else(|_| chrono::NaiveDate::parse_from_str(&value, \"%Y-%m-%d\")).map(|date| date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64)).unwrap_or_else(|_| value.parse::<f64>().unwrap_or(f64::NAN)), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
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
            "{{ let year_value = ({year}) as f64; let month_value = ({month}) as f64; let day_value = ({day}) as f64; let hour_value = ({hour}) as f64; let minute_value = ({minute}) as f64; let second_value = ({second}) as f64; let milli_value = ({milli}) as f64; if [year_value, month_value, day_value, hour_value, minute_value, second_value, milli_value].into_iter().all(f64::is_finite) {{ let year = year_value as i32; let month0 = month_value as u32; let day = day_value as u32; let hour = hour_value as u32; let minute = minute_value as u32; let second = second_value as u32; let milli = milli_value as u32; chrono::NaiveDate::from_ymd_opt(year, month0 + 1, day).and_then(|date| date.and_hms_milli_opt(hour, minute, second, milli)).map(|dt| dt.and_utc().timestamp_millis()).unwrap_or(i64::MIN) }} else {{ i64::MIN }} }}",
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
            "{{ use {trait_use}; let timestamp_ms = ({timestamp_text}) as f64; if timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map_or(f64::NAN, |date| {accessor}) }} else {{ f64::NAN }} }}"
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
        let values_are_finite = value_texts
            .iter()
            .map(|value| format!("(({value}) as f64).is_finite()"))
            .collect::<Vec<_>>()
            .join(" && ");
        let update = match part {
            smelt_hir::DatePart::FullYear => {
                let Some(year) = value_texts.first() else {
                    return Err(EmitError::new("Date.setFullYear requires a year value"));
                };
                let month = value_texts.get(1).map_or("date.month0()", String::as_str);
                let day = value_texts.get(2).map_or("date.day()", String::as_str);
                format!(
                    "{{ let month_index = {month} as i32; let normalized_year = ({year} as i32) + month_index.div_euclid(12); let normalized_month0 = month_index.rem_euclid(12) as u32; chrono::NaiveDate::from_ymd_opt(normalized_year, normalized_month0 + 1, 1).and_then(|base| base.and_hms_nano_opt(date.hour(), date.minute(), date.second(), date.nanosecond())).map(|base| base + chrono::Duration::days(({day} as i64) - 1)).map(|date| date.and_utc()) }}"
                )
            }
            smelt_hir::DatePart::Month => {
                let Some(month) = value_texts.first() else {
                    return Err(EmitError::new("Date.setMonth requires a month value"));
                };
                let day = value_texts.get(1).map_or("date.day()", String::as_str);
                format!(
                    "{{ let month_index = {month} as i32; let normalized_year = date.year() + month_index.div_euclid(12); let normalized_month0 = month_index.rem_euclid(12) as u32; chrono::NaiveDate::from_ymd_opt(normalized_year, normalized_month0 + 1, 1).and_then(|base| base.and_hms_nano_opt(date.hour(), date.minute(), date.second(), date.nanosecond())).map(|base| base + chrono::Duration::days(({day} as i64) - 1)).map(|date| date.and_utc()) }}"
                )
            }
            smelt_hir::DatePart::Date => {
                let Some(day) = value_texts.first() else {
                    return Err(EmitError::new("Date.setDate requires a day value"));
                };
                format!(
                    "chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), 1).and_then(|base| base.and_hms_nano_opt(date.hour(), date.minute(), date.second(), date.nanosecond())).map(|base| base + chrono::Duration::days(({day} as i64) - 1)).map(|date| date.and_utc())"
                )
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
                let milli = value_texts
                    .get(3)
                    .map_or("date.timestamp_subsec_millis() as f64", String::as_str);
                format!(
                    "chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()).and_then(|base| base.and_hms_milli_opt(0, 0, 0, 0)).map(|base| base + chrono::Duration::milliseconds((({hour} as i64) * 3_600_000) + (({minute} as i64) * 60_000) + (({second} as i64) * 1_000) + ({milli} as i64))).map(|date| date.and_utc())"
                )
            }
            smelt_hir::DatePart::Minute => {
                let Some(minute) = value_texts.first() else {
                    return Err(EmitError::new("Date.setMinutes requires a minute value"));
                };
                let second = value_texts.get(1).map_or("date.second()", String::as_str);
                let milli = value_texts
                    .get(2)
                    .map_or("date.timestamp_subsec_millis() as f64", String::as_str);
                format!(
                    "chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()).and_then(|base| base.and_hms_milli_opt(date.hour(), 0, 0, 0)).map(|base| base + chrono::Duration::milliseconds((({minute} as i64) * 60_000) + (({second} as i64) * 1_000) + ({milli} as i64))).map(|date| date.and_utc())"
                )
            }
            smelt_hir::DatePart::Second => {
                let Some(second) = value_texts.first() else {
                    return Err(EmitError::new("Date.setSeconds requires a second value"));
                };
                let milli = value_texts
                    .get(1)
                    .map_or("date.timestamp_subsec_millis() as f64", String::as_str);
                format!(
                    "chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()).and_then(|base| base.and_hms_milli_opt(date.hour(), date.minute(), 0, 0)).map(|base| base + chrono::Duration::milliseconds((({second} as i64) * 1_000) + ({milli} as i64))).map(|date| date.and_utc())"
                )
            }
            smelt_hir::DatePart::Millisecond => {
                let Some(milli) = value_texts.first() else {
                    return Err(EmitError::new(
                        "Date.setMilliseconds requires a millisecond value",
                    ));
                };
                format!(
                    "chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()).and_then(|base| base.and_hms_milli_opt(date.hour(), date.minute(), date.second(), 0)).map(|base| base + chrono::Duration::milliseconds({milli} as i64)).map(|date| date.and_utc())"
                )
            }
        };
        Ok(format!(
            "{{ use chrono::{{Datelike as _, Timelike as _}}; let timestamp_ms = ({timestamp_text}) as f64; if timestamp_ms.is_finite() && {values_are_finite} {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).and_then(|date| {update}).map(|date| date.timestamp_millis()).unwrap_or(i64::MIN) }} else {{ i64::MIN }} }}"
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
            smelt_hir::UrlField::Origin => format!(
                "{{ let url = {parsed}; url.host_str().map(|host| match url.port() {{ Some(port) => format!(\"{{}}://{{}}:{{}}\", url.scheme(), host, port), None => format!(\"{{}}://{{}}\", url.scheme(), host) }}).unwrap_or_default() }}"
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
