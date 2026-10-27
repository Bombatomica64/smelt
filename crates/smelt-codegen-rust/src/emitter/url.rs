//! Emission of WHATWG `URL` operations on the concrete `SmeltUrl` runtime type.
//!
//! The runtime type and its throwing adapters live in `url_prelude`. This
//! module renders the MIR that uses them:
//!
//! * [`Rvalue::UrlOp`](smelt_mir::Rvalue::UrlOp): a component read
//!   (`url.pathname()`), an infallible component write, or `searchParams`;
//! * the fallible calls `BuiltinFn::UrlNew` and `BuiltinFn::UrlSetHref`, whose
//!   trailing `?` is what marks them throwing to the call-terminator emitter.

use smelt_hir::{Type, TypeId, UrlField, UrlOp};
use smelt_mir::Operand;

use super::{EmitError, FunctionEmitter};
use crate::url_prelude::{URL_NEW_FN, URL_SET_HREF_FN};

impl FunctionEmitter<'_> {
    /// The `SmeltUrl` method that reads a component.
    const fn url_getter(field: UrlField) -> &'static str {
        match field {
            UrlField::Href => "href",
            UrlField::Origin => "origin",
            UrlField::Protocol => "protocol",
            UrlField::Username => "username",
            UrlField::Password => "password",
            UrlField::Host => "host",
            UrlField::Hostname => "hostname",
            UrlField::Port => "port",
            UrlField::Pathname => "pathname",
            UrlField::Search => "search",
            UrlField::Hash => "hash",
        }
    }

    /// Render a `URL` component read, infallible write, or `searchParams`.
    ///
    /// A write answers the ASSIGNED text, which is what the source assignment
    /// expression evaluates to. The `href` write never reaches here: it throws,
    /// so MIR lowers it to `BuiltinFn::UrlSetHref`.
    pub(super) fn url_op_text(
        &self,
        op: UrlOp,
        url: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        // Every `SmeltUrl` method takes `&self`: the handle is shared, so a
        // write through the borrow is observed by every alias.
        let receiver = self.operand_borrow_text(url)?;
        let string_ty = self.type_id(Type::String)?;
        let scope = self.render_scope();
        match op {
            UrlOp::Get(field) => {
                let text = format!("{receiver}.{}()", Self::url_getter(field));
                self.value_at_type_text(&text, string_ty, dest_ty, &scope)
            }
            UrlOp::Set(field) => {
                if !field.is_settable() || field == UrlField::Href {
                    return Err(EmitError::new(format!(
                        "`URL.{}` has no infallible setter",
                        Self::url_getter(field)
                    )));
                }
                let value = args
                    .first()
                    .ok_or_else(|| EmitError::new("a `URL` setter takes one value"))?;
                let value_text = self.value_at_type(value, string_ty)?;
                let text = format!(
                    "{{ let smelt_url_value: String = {value_text}; {receiver}.set_{}(&smelt_url_value); smelt_url_value }}",
                    Self::url_getter(field)
                );
                self.value_at_type_text(&text, string_ty, dest_ty, &scope)
            }
            UrlOp::SearchParams => {
                let params_ty = self
                    .stdlib_class_ty(smelt_stdlib::StdlibClass::UrlSearchParams)
                    .ok_or_else(|| {
                        EmitError::new("`URL.searchParams` needs the URLSearchParams type interned")
                    })?;
                self.value_at_type_text(
                    &format!("{receiver}.search_params()"),
                    params_ty,
                    dest_ty,
                    &scope,
                )
            }
        }
    }

    /// Render the fallible `new URL(input, base?)` call.
    pub(super) fn url_new_call_text(&self, args: &[Operand]) -> Result<String, EmitError> {
        let input = args
            .first()
            .ok_or_else(|| EmitError::new("`new URL` takes an input"))?;
        let input_text = self.string_like_operand_text(input, "URL input")?;
        let base_text = match args.get(1) {
            Some(base) => format!(
                "Some({}.as_str())",
                self.string_like_operand_text(base, "URL base")?
            ),
            None => "None".to_owned(),
        };
        Ok(format!("{URL_NEW_FN}({input_text}.as_str(), {base_text})?"))
    }

    /// Render the fallible `url.href = text` call.
    pub(super) fn url_set_href_call_text(&self, args: &[Operand]) -> Result<String, EmitError> {
        let [url, value] = args else {
            return Err(EmitError::new("`URL.href` assignment takes a URL and a value"));
        };
        let url_text = self.operand_borrow_text(url)?;
        let value_text = self.string_like_operand_text(value, "URL href")?;
        Ok(format!("{URL_SET_HREF_FN}(&{url_text}, {value_text})?"))
    }

    /// The type a fallible `URL` call answers: the `URL` class for the
    /// constructor, the assigned string for the `href` setter.
    pub(super) fn url_call_source_ty(&self, constructs: bool) -> Result<TypeId, EmitError> {
        if constructs {
            self.stdlib_class_ty(smelt_stdlib::StdlibClass::Url)
                .ok_or_else(|| EmitError::new("`new URL` needs the URL class type interned"))
        } else {
            self.type_id(Type::String)
        }
    }
}
