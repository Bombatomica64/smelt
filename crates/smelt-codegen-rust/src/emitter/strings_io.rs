//! Strings Io emission helpers.

use super::*;

impl FunctionEmitter<'_> {

    /// Converts a timestamp in milliseconds to an RFC 3339 timestamp string.
    pub(super) fn date_to_iso_string_text(&self, timestamp_ms: &Operand) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(timestamp_ms)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("date timestamp must be numeric"));
        }
        let timestamp_text = self.operand_text(timestamp_ms)?;
        Ok(format!(
            "chrono::DateTime::<chrono::Utc>::from_timestamp_millis({timestamp_text} as i64).expect(\"timestamp out of range\").to_rfc3339_opts(chrono::SecondsFormat::Millis, true)"
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
    pub(super) fn file_write_text(&self, path: &Operand, text: &Operand) -> Result<String, EmitError> {
        self.require_string_operands(&[path, text], "file write")?;
        let path_text = self.operand_text(path)?;
        let text_text = self.operand_text(text)?;
        Ok(format!(
            "{{ let contents = {text_text}; std::fs::write(&{path_text}, &contents).expect(\"file write failed\"); contents.len() as i64 }}"
        ))
    }

    // Checks that every operand has string type.

}
