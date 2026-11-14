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
    pub(super) fn py_text_expr(
        &self,
        value: &Operand,
        method: &str,
    ) -> Result<RustExpr, EmitError> {
        if !self.type_has_py_repr(self.operand_ty(value)?) {
            return Err(EmitError::new(
                "Python str()/repr() of a value with no Python text",
            ));
        }
        let text = self.operand_borrow_expr(value)?;
        Ok(RustExpr::atom(format!(
            "SmeltPyRepr::{method}(&{text})",
            text = text.tight()
        )))
    }

    /// `json.dumps(value)`: the runtime's `SmeltPyJson` encoding of the value
    /// at its own static type (`runtime/py_repr.rs`), never through the
    /// erased carrier, which cannot tell an `int` from a `float`.
    pub(super) fn py_json_dumps_expr(&self, value: &Operand) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(value)?;
        if !self.type_has_py_json(ty) {
            return Err(EmitError::new(format!(
                "json.dumps() of a value with no Python JSON form, got {}",
                self.rendered_type(ty)?
            )));
        }
        let text = self.operand_borrow_expr(value)?;
        Ok(RustExpr::atom(format!(
            "smelt_py_json_dumps(&{text})",
            text = text.tight()
        )))
    }

    /// Whether a value of type `ty` has a `SmeltPyJson` impl: Python's
    /// scalars, `None`, and lists, tuples and hash-map dicts of such values,
    /// with `str`, `int`, `float` or `bool` keys.
    fn type_has_py_json(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(
                Type::Int | Type::Float | Type::Bool | Type::PyStr | Type::String | Type::None,
            ) => true,
            Some(Type::Optional(inner) | Type::List(inner)) => self.type_has_py_json(*inner),
            Some(Type::Tuple(items)) => {
                !items.is_empty()
                    && items.len() <= 6
                    && items.iter().all(|item| self.type_has_py_json(*item))
            }
            Some(Type::Dict(key, value)) => {
                !self.dict_uses_js_key_map(*key)
                    && matches!(
                        self.mir.types.get(*key),
                        Some(Type::PyStr | Type::String | Type::Int | Type::Float | Type::Bool)
                    )
                    && self.type_has_py_json(*value)
            }
            _ => false,
        }
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
            .map(|arg| {
                Ok(format!(
                    "SmeltPyException::arg(&{})",
                    self.operand_expr(arg)?.tight()
                ))
            })
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
