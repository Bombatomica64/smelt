// Source bindings that can be read before their first assignment.
//
// A Python name assigned on only some paths, or a JavaScript `let` a closure
// reads in its temporal dead zone, has no value until its assignment runs,
// and reading it then raises. MIR marks exactly those bindings
// (`LocalInit::MaybeUnbound`); every other binding keeps its plain type. A
// marked binding is stored as a `SmeltBinding<T>`, which knows it is empty,
// and read through `bound`, only where the definite-assignment analysis proved
// it assigned; before a read it cannot prove, a `checked` call raises the
// source language's error on the same error channel as any other throw.

// @smelt:item SmeltBinding when binding
/// A source binding that may not be assigned yet.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct SmeltBinding<T>(Option<T>);

#[allow(dead_code)]
impl<T> SmeltBinding<T> {
    /// A binding before its first assignment.
    const fn unbound() -> Self { Self(None) }
    /// Assigns the binding.
    fn bind(&mut self, value: T) { self.0 = Some(value); }
    /// Reads a binding the compiler proved assigned at this read.
    fn bound(&self) -> &T { self.0.as_ref().unwrap_or_else(|| unreachable!("a binding proved assigned was read unassigned")) }
    /// Writes through a binding the compiler proved assigned at this write.
    fn bound_mut(&mut self) -> &mut T { self.0.as_mut().unwrap_or_else(|| unreachable!("a binding proved assigned was written unassigned")) }
    /// Raises `unbound()` if the binding is not assigned.
    fn checked(&self, unbound: impl FnOnce() -> Box<dyn ::std::error::Error>) -> Result<&T, Box<dyn ::std::error::Error>> { self.0.as_ref().ok_or_else(unbound) }
}

// @smelt:item-end
// @smelt:item smelt_js_tdz_error when binding_tdz
/// The `ReferenceError` a binding read in its temporal dead zone throws.
#[allow(dead_code)]
fn smelt_js_tdz_error(name: &str) -> Box<dyn ::std::error::Error> { smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("ReferenceError".into())), ("message".to_owned(), SmeltUnknown::String(format!("Cannot access '{name}' before initialization").into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))) }

// @smelt:item-end
// @smelt:item smelt_py_unbound_local_error when binding_py
/// The `UnboundLocalError` a function local read before assignment raises.
#[allow(dead_code)]
fn smelt_py_unbound_local_error(name: &str) -> Box<dyn ::std::error::Error> { SmeltPyException::new(&["UnboundLocalError", "NameError", "Exception", "BaseException"], vec![SmeltPyException::arg(&format!("cannot access local variable '{name}' where it is not associated with a value"))]).into_error() }

/// The `NameError` a free variable read before its enclosing function assigned it raises.
#[allow(dead_code)]
fn smelt_py_unbound_free_error(name: &str) -> Box<dyn ::std::error::Error> { SmeltPyException::new(&["NameError", "Exception", "BaseException"], vec![SmeltPyException::arg(&format!("cannot access free variable '{name}' where it is not associated with a value in enclosing scope"))]).into_error() }

/// The `NameError` a module-level name read before it is bound raises.
#[allow(dead_code)]
fn smelt_py_name_error(name: &str) -> Box<dyn ::std::error::Error> { SmeltPyException::new(&["NameError", "Exception", "BaseException"], vec![SmeltPyException::arg(&format!("name '{name}' is not defined"))]).into_error() }

// @smelt:item-end
