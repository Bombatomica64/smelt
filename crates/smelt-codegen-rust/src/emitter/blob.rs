//! Rust emission for the WHATWG `Blob`/`File` value.
//!
//! Every member lands on a real method of the concrete `SmeltBlob` emitted by
//! `crate::blob_prelude`, so `blob.size` is `blob.size()` typed `f64` and
//! `await blob.text()` is a real future over the blob's bytes — no tagged
//! record and no runtime member lookup in between.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit `new Blob(parts?, options?)` / `new File(parts, name, options?)`.
    ///
    /// `BlobPart` is `Blob | BufferSource | string`, and two of the three arms
    /// are modeled concretely — so the parts array arrives already typed as a
    /// list of strings or a list of blobs whenever the source spelled one of
    /// those, and each gets its own typed constructor. Only a genuinely
    /// heterogeneous array (mixed arms, or a `BufferSource`, still the erased
    /// byte-backed host record family) arrives erased and goes through the
    /// boundary adapter `SmeltBlob::from_parts_unknown`, which is also the one
    /// the reflected host constructor uses.
    pub(super) fn blob_new_text(
        &self,
        parts: &Operand,
        blob_type: &Operand,
        name: Option<&Operand>,
        last_modified: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let parts_text = self.operand_text(parts)?;
        let type_text = self.operand_text(blob_type)?;
        let parts_ty = self.operand_ty(parts)?;
        let concrete_constructor = match self.mir.types.get(parts_ty) {
            Some(&Type::List(item)) => {
                if matches!(self.mir.types.get(item), Some(Type::String)) {
                    Some("from_string_parts")
                } else if self.is_blob_class_type(item)? {
                    Some("from_blob_parts")
                } else {
                    None
                }
            }
            _ => None,
        };
        let name_text = match name {
            Some(name_operand) => format!("Some(({}).clone())", self.operand_text(name_operand)?),
            None => "None".to_owned(),
        };
        let last_modified_text = match last_modified {
            Some(operand) => format!("Some(({}) as f64)", self.operand_text(operand)?),
            None => "None".to_owned(),
        };
        if let Some(constructor) = concrete_constructor {
            return Ok(format!(
                "SmeltBlob::{constructor}(({parts_text}).clone().into(), ({type_text}).clone(), {name_text}, {last_modified_text})"
            ));
        }
        Ok(format!(
            "SmeltBlob::from_parts_unknown(({parts_text}).clone(), ({type_text}).clone(), {name_text}, {last_modified_text})"
        ))
    }

    /// Emit one `Blob`/`File` member as a method call.
    pub(super) fn blob_op_text(
        &self,
        op: smelt_hir::BlobOp,
        blob: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(blob)?;
        match op {
            smelt_hir::BlobOp::Size => {
                let float_ty = self.type_id(Type::Float)?;
                self.value_at_type_text(&format!("{receiver}.size()"), float_ty, dest_ty)
            }
            smelt_hir::BlobOp::LastModified => {
                let float_ty = self.type_id(Type::Float)?;
                self.value_at_type_text(&format!("{receiver}.last_modified()"), float_ty, dest_ty)
            }
            smelt_hir::BlobOp::Type => {
                let string_ty = self.type_id(Type::String)?;
                self.value_at_type_text(&format!("{receiver}.blob_type()"), string_ty, dest_ty)
            }
            smelt_hir::BlobOp::Name => {
                let string_ty = self.type_id(Type::String)?;
                self.value_at_type_text(&format!("{receiver}.file_name()"), string_ty, dest_ty)
            }
            // The body readers are `async` in the spec even though a blob's
            // bytes are already in memory: the source awaits them, so the value
            // has to be a real future or the `await` has nothing to poll.
            smelt_hir::BlobOp::Text => Ok(format!(
                "{{ let smelt_blob = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>(smelt_blob.to_text()) }})) }}"
            )),
            smelt_hir::BlobOp::ArrayBuffer | smelt_hir::BlobOp::Bytes => Ok(format!(
                "{{ let smelt_blob = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>(SmeltUint8Array::from_bytes(smelt_blob.to_bytes())) }})) }}"
            )),
            smelt_hir::BlobOp::Slice => {
                let float_ty = self.type_id(Type::Float)?;
                let string_ty = self.type_id(Type::String)?;
                let range = |index: usize| -> Result<String, EmitError> {
                    match args.get(index) {
                        None => Ok("None".to_owned()),
                        Some(arg) => Ok(format!("Some({})", self.value_at_type(arg, float_ty)?)),
                    }
                };
                let content_type = match args.get(2) {
                    None => "None".to_owned(),
                    Some(arg) => format!("Some({})", self.value_at_type(arg, string_ty)?),
                };
                Ok(format!(
                    "{receiver}.slice({}, {}, {content_type})",
                    range(0)?,
                    range(1)?
                ))
            }
        }
    }

    /// Return whether a type names the generated `SmeltBlob` type.
    pub(super) fn is_blob_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self
            .stdlib_class_of_symbol(*name)?
            .is_some_and(smelt_stdlib::StdlibClass::is_blob_runtime_type))
    }
}
