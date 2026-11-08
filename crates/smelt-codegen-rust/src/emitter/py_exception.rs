//! Emission of Python exception values: construction, the `except` match, and
//! the class-chain literal both of them name.
//!
//! Every Python exception is one `SmeltPyException` (see the runtime's
//! `py_exception.rs`), so these operations only ever deal in class names: the
//! frontend resolved which classes a `raise` builds and an `except` names.

use crate::rust::RustExpr;
use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::Operand;

use super::{EmitError, FunctionEmitter};

impl FunctionEmitter<'_> {
    /// `SmeltPyRepr::py_str(&value)` or `py_repr`: Python's `str` or `repr`
    /// of a value whose type the runtime's `SmeltPyRepr` writes.
    pub(super) fn py_text_expr(&self, value: &Operand, method: &str) -> Result<RustExpr, EmitError> {
        if !self.type_has_py_repr(self.operand_ty(value)?) {
            return Err(EmitError::new("Python str()/repr() of a value with no Python text"));
        }
        let text = self.operand_borrow_expr(value)?;
        Ok(RustExpr::atom(format!("SmeltPyRepr::{method}(&{text})", text = text.tight())))
    }

    /// Whether `ty` is a Python exception, rendered as `SmeltPyException`.
    pub(super) fn is_py_exception_type(&self, ty: TypeId) -> bool {
        self.py_stdlib_class_of(ty) == Some(smelt_stdlib::StdlibClass::PyException)
    }

    /// Whether `ty` is a Python exception's `args`, `SmeltPyExceptionArgs`.
    pub(super) fn is_py_exception_args_type(&self, ty: TypeId) -> bool {
        self.py_stdlib_class_of(ty) == Some(smelt_stdlib::StdlibClass::PyExceptionArgs)
    }

    /// The modeled stdlib class a class type `ty` names, if any.
    fn py_stdlib_class_of(&self, ty: TypeId) -> Option<smelt_stdlib::StdlibClass> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        crate::stdlib::stdlib_class_of_class_symbol(self.mir, *name)
    }

    /// The `&'static [&'static str]` literal of the class names `classes`.
    fn py_class_names_literal(&self, classes: &[Symbol]) -> Result<String, EmitError> {
        let names = classes
            .iter()
            .map(|class| self.symbol_name(*class).map(|name| format!("{name:?}")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(format!("&[{}]", names.join(", ")))
    }

    /// `SmeltPyException::new(chain, args)`: an instance of the class whose
    /// chain is `classes`, keeping each argument's `str` and `repr`.
    pub(super) fn py_exception_new_expr(
        &self,
        classes: &[Symbol],
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let chain = self.py_class_names_literal(classes)?;
        let arg_texts = args
            .iter()
            .map(|arg| Ok(format!("SmeltPyException::arg(&{})", self.operand_expr(arg)?.tight())))
            .collect::<Result<Vec<_>, EmitError>>()?;
        Ok(RustExpr::atom(format!(
            "SmeltPyException::new({chain}, vec![{}])",
            arg_texts.join(", ")
        )))
    }

    /// Whether the `except` clause naming `classes` catches `exception`.
    pub(super) fn py_exception_caught_by_expr(
        &self,
        exception: &Operand,
        classes: &[Symbol],
        catches_unclassified: bool,
    ) -> Result<RustExpr, EmitError> {
        let exception_text = self.operand_expr(exception)?;
        let names = self.py_class_names_literal(classes)?;
        Ok(exception_text.postfix(format!(".caught_by({names}, {catches_unclassified})")))
    }
}
