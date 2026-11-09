//! Arguments the erased prototype adapter (`class_proto`) passes to a typed
//! method, for the parameters the blanket `SmeltFromUnknown` cannot rebuild.
//!
//! A method read off an erased instance is called through the erased call
//! ABI, which hands the adapter a `Vec<SmeltUnknown>`. A data parameter is
//! rebuilt through `SmeltFromUnknown`; a CALLBACK parameter (`otherwise(handler:
//! (value: unknown) => unknown)`) and a REST parameter (`with(...args)`) have no
//! such impl, so the adapter used to leave the whole method off the prototype
//! and the erased instance answered `undefined` (a default `null` once the
//! value was rebuilt as a record). The method's own emitter knows how each such
//! parameter is spelled in its signature (`parameter_decl_type`: a borrowed
//! `&dyn Fn`, a generic `&F0`, an owned `Rc<dyn Fn>`, `&mut`), so it renders
//! the argument at exactly that type.

use super::FunctionEmitter;
use crate::EmitError;
use crate::rust::RustExpr;
use smelt_mir::LocalId;

impl FunctionEmitter<'_> {
    /// The argument for the method parameter `param`, extracted from the erased
    /// value `erased` (an expression of type `SmeltUnknown`) and borrowed the
    /// way the method's signature takes it.
    ///
    /// A callback extracts to an owned `Rc<dyn Fn ..>` that calls the erased
    /// function (`extract_function`); a borrowed callback parameter takes
    /// `&*` of it, which a `&dyn Fn` and a `&F0` (`F0: Fn + ?Sized`) both
    /// accept. A rest list extracts element-wise from the erased array the
    /// adapter packs the remaining arguments into.
    pub(crate) fn erased_proto_argument(
        &self,
        param: LocalId,
        erased: &str,
    ) -> Result<String, EmitError> {
        let ty = self.local_decl(param)?.ty;
        let declared = self.parameter_decl_type(param)?.into_string();
        let scope = self.render_scope();
        let value = self.extract_rendered(&RustExpr::atom(erased), ty, &scope)?;
        Ok(if declared.starts_with("&mut ") {
            format!("&mut ({value})", value = value.delimited())
        } else if declared.starts_with('&')
            && matches!(self.mir.types.get(ty), Some(smelt_hir::Type::Function(_)))
        {
            format!("&*({value})", value = value.delimited())
        } else if declared.starts_with('&') {
            format!("&({value})", value = value.delimited())
        } else {
            value.into_string()
        })
    }
}
