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

/// The `impl SmeltPyExceptionObject for <class>` text of a Python exception
/// class (`MirClassProtocol::PyException`, `docs/design/python-exceptions.md`).
///
/// The class is a reference class (`crate::classify`), so its instance is one
/// shared cell: `smelt_any` hands that cell out for the checked downcast an
/// `except C as e` binds, and `py_classes` answers the chain of the cell's
/// runtime class, tested by the same `Any::is` probes, deepest class first.
/// The texts are the class's `SmeltPyRepr` impl (its `__str__`/`__repr__`,
/// which the frontend gives every exception root), and `py_args` calls the
/// synthesized `args` getter.
#[expect(
    clippy::too_many_arguments,
    reason = "one class impl and its rendering context"
)]
pub(crate) fn python_exception_impl_text(
    mir: &Mir,
    context: &EmitContext,
    class: Symbol,
    class_name: &str,
    impl_generics: &str,
    type_args: &str,
    builtin_base: Symbol,
    args_getter: &MirFunction,
) -> Result<String, EmitError> {
    if !context.is_reference_class(class) {
        return Err(EmitError::new(
            "a Python exception class must be a reference class",
        ));
    }
    let emitter = FunctionEmitter::new(mir, context, args_getter)?;
    let HirOrigin::ClassMethod { method, .. } = args_getter.origin else {
        return Err(EmitError::new(
            "a Python exception's `args` getter is not an instance method",
        ));
    };
    let args_call = format!("self.{}()", emitter.method_rust_name(method)?);
    let builtin = mir
        .symbols
        .get(builtin_base)
        .ok_or_else(|| EmitError::new("a Python exception's builtin base has no name"))?;
    let builtin_chain = smelt_stdlib::py_exceptions::builtin_exception_chain(builtin)
        .ok_or_else(|| EmitError::new(format!("`{builtin}` is not a builtin exception class")))?;
    let chain_literal = |concrete: Symbol| -> Result<String, EmitError> {
        let mut names = Vec::new();
        let mut current = Some(concrete);
        let mut seen = std::collections::HashSet::new();
        while let Some(symbol) = current {
            if !seen.insert(symbol) {
                break;
            }
            let name = mir
                .symbols
                .get(symbol)
                .ok_or_else(|| EmitError::new("a Python exception class has no name"))?;
            names.push(format!("{name:?}"));
            current = crate::classes::find_class(mir, symbol).and_then(|found| found.base);
        }
        names.extend(builtin_chain.iter().map(|name| format!("{name:?}")));
        Ok(format!("&[{}]", names.join(", ")))
    };
    let polymorphic = context.is_polymorphic_class(class);
    let (any_text, classes_text) = if polymorphic {
        let mut probes = String::new();
        let concrete =
            crate::class_polymorphism::concrete_classes(mir, context.polymorphic_classes(), class);
        for candidate in concrete
            .iter()
            .rev()
            .filter(|candidate| **candidate != class)
        {
            let inner = format!("{}Inner", emitter.type_rust_name(*candidate)?);
            probes.push_str(&format!(
                "if smelt_any.is::<::std::cell::RefCell<{inner}>>() {{ return {}; }} ",
                chain_literal(*candidate)?
            ));
        }
        (
            "::std::rc::Rc::clone(&self.0).smelt_any()".to_owned(),
            format!(
                "let smelt_any = SmeltPyExceptionObject::smelt_any(self); {probes}let _ = smelt_any; {}",
                chain_literal(class)?
            ),
        )
    } else {
        (
            "{ let smelt_cell: ::std::rc::Rc<dyn ::std::any::Any> = self.0.clone(); smelt_cell }"
                .to_owned(),
            chain_literal(class)?,
        )
    };
    Ok(format!(
        "\nimpl{impl_generics} SmeltPyExceptionObject for {class_name}{type_args} {{\n    fn smelt_any(&self) -> ::std::rc::Rc<dyn ::std::any::Any> {{ {any_text} }}\n    fn py_classes(&self) -> &'static [&'static str] {{ {classes_text} }}\n    fn py_args(&self) -> SmeltPyExceptionArgs {{ {args_call} }}\n    fn py_text(&self) -> String {{ SmeltPyRepr::py_str(self) }}\n    fn py_text_repr(&self) -> String {{ SmeltPyRepr::py_repr(self) }}\n}}\n"
    ))
}
