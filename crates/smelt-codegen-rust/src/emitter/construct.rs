//! Emission for JavaScript `[[Construct]]` and value-target `instanceof`.
//!
//! Both nodes ask a question about a function value that only the runtime can
//! answer — what the callee's `prototype` object is, and what a value's
//! prototype chain reaches — so both lower to one prelude helper each
//! (`smelt_construct`, `smelt_instance_of_value`; see
//! `crate::function_object_prelude`). The emitter's whole job here is to erase
//! the operands to `SmeltUnknown`, which is the honest ABI: the constructor is
//! reached through a value whose identity is only known at runtime, and the
//! constructed object's shape is whatever the constructor decided.

use smelt_hir::TypeId;
use smelt_mir::Operand;

use super::{EmitError, FunctionEmitter};

impl FunctionEmitter<'_> {
    /// Render JavaScript `new callee(args)` through a function value.
    ///
    /// The callee is erased because construction reaches it as a VALUE: a
    /// typed `Rc<dyn Fn(..)>` and a `SmeltUnknown::Function` are the same
    /// JavaScript function, and the runtime helper needs the erased form to
    /// read its `prototype` property and to install a receiver for the call.
    /// The construction itself yields `SmeltUnknown` — a constructor decides
    /// its own result — so the value is extracted at the destination type,
    /// which is what lets `new f()` sit in a statement position (`()`) or feed
    /// a concretely typed binding.
    pub(super) fn construct_text(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let callee_text = self.erase(callee)?;
        let arg_texts = args
            .iter()
            .map(|arg| self.erase(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let call_text = format!("smelt_construct({callee_text}, ::std::vec![{arg_texts}])");
        self.extract_value_text(&call_text, dest_ty)
    }

    /// Render JavaScript `value instanceof target` for a runtime constructor.
    ///
    /// Unlike [`FunctionEmitter::instance_of_text`], whose target is a class
    /// name known at compile time, the target here is a value: the answer is a
    /// prototype-chain walk, so both operands are erased and handed to the
    /// runtime predicate.
    pub(super) fn instance_of_value_text(
        &self,
        value: &Operand,
        target: &Operand,
    ) -> Result<String, EmitError> {
        let value_text = self.erase(value)?;
        let target_text = self.erase(target)?;
        Ok(format!(
            "smelt_instance_of_value(&{value_text}, &{target_text})"
        ))
    }
}
