//! A Python class's text: `SmeltPyRepr` impls for classes that define
//! `__repr__` (and optionally `__str__`).
//!
//! `repr(x)`, `str(x)` and `print(x)` of a class instance call its method
//! directly (the frontend lowers them to the method call). A class instance
//! INSIDE a printed container (`print([x])`, `repr((x, 1))`) is written by the
//! container's `SmeltPyRepr` impl, which asks each element for its `repr`; this
//! module gives such a class the impl, delegating to its own methods:
//!
//! * `py_repr` calls `__repr__`;
//! * `py_str` calls `__str__`, or `__repr__` when the class has no `__str__`
//!   (`object.__str__` is `repr`).
//!
//! A class with only `__str__` gets no impl: its `repr` is `CPython`'s
//! `<__main__.C object at 0x...>`, an address no port can reproduce, so the
//! frontend refuses to put one where its `repr` would be shown.
//!
//! A raising text method re-raises through the panic route, the channel a
//! raising callback of a builtin uses; `SmeltPyRepr` answers a plain `String`.

use smelt_hir::Symbol;
use smelt_mir::{HirOrigin, Mir, MirFunction};

use super::{EmitContext, EmitError, FunctionEmitter};

/// The `impl SmeltPyRepr for <class>` text, or nothing when the class has no
/// `__repr__`.
pub(crate) fn python_text_impl_text(
    mir: &Mir,
    context: &EmitContext,
    class_name: &str,
    impl_generics: &str,
    type_args: &str,
    str_method: Option<&MirFunction>,
    repr: Option<&MirFunction>,
) -> Result<String, EmitError> {
    let Some(repr_method) = repr else {
        return Ok(String::new());
    };
    let call = |function: &MirFunction| -> Result<String, EmitError> {
        let emitter = FunctionEmitter::new(mir, context, function)?;
        let HirOrigin::ClassMethod { method, .. } = function.origin else {
            return Err(EmitError::new(
                "a Python __str__/__repr__ protocol target is not an instance method",
            ));
        };
        let name = emitter.method_rust_name(method)?;
        Ok(if function.can_throw {
            format!("self.{name}().unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error))")
        } else {
            format!("self.{name}()")
        })
    };
    let repr_call = call(repr_method)?;
    let str_call = str_method
        .map(call)
        .transpose()?
        .unwrap_or_else(|| repr_call.clone());
    Ok(format!(
        "\nimpl{impl_generics} SmeltPyRepr for {class_name}{type_args} {{\n    fn py_repr(&self) -> String {{ {repr_call} }}\n    fn py_str(&self) -> String {{ {str_call} }}\n}}\n"
    ))
}

/// Whether the class named `class` has a `SmeltPyRepr` impl (a `__repr__`).
pub(crate) fn class_has_python_repr(mir: &Mir, class: Symbol) -> bool {
    mir.classes.iter().any(|candidate| {
        candidate.name == class
            && candidate
                .protocols
                .iter()
                .any(|protocol| matches!(protocol, smelt_mir::MirClassProtocol::Repr { .. }))
    })
}
