// The identity of the function a record was built from (design decision C1,
// `docs/design/record-identity.md`).
//
// TypeScript accepts a function where an object type is expected
// (`const n: Named = hello`, `interface Named { name: string; length: number }`)
// and JavaScript keeps ONE object: `n === hello` is `true`. A Rust port builds
// the record from the function's `name` and `length`, so the record is a second
// value; this token is how it stays the same JavaScript object for `===` and
// `Object.is`:
//
// * the key is the function's allocation address, the same key two function
//   values already compare by (`Rc::as_ptr`, `smelt_callable_object_key`);
// * a `Weak` to that allocation keeps the address from being reused while the
//   record lives, so a record outliving every other handle on its function
//   never compares equal to a later function allocated at the same address.
//   The weak handle keeps no captured value alive and so forms no `Rc` cycle;
// * it is not an own property: keys, JSON, inspection and deep equality never
//   see it, so two tokens are always `==` for derived equality.
//
// A record that no function value built carries the empty token.

// @smelt:item SmeltFunctionToken when function_tokens
/// The function a record built from a function value stands for: its identity
/// key, and a weak handle that pins the key's allocation.
#[derive(Default)]
struct SmeltFunctionToken(Option<(usize, Box<dyn SmeltFunctionAllocation>)>);
/// A weak handle on a function allocation of any signature, clonable behind a box.
trait SmeltFunctionAllocation { fn clone_box(&self) -> Box<dyn SmeltFunctionAllocation>; }
impl<F: ?Sized + 'static> SmeltFunctionAllocation for ::std::rc::Weak<F> { fn clone_box(&self) -> Box<dyn SmeltFunctionAllocation> { Box::new(self.clone()) } }
#[allow(dead_code)]
impl SmeltFunctionToken {
    /// The token of the function `function`.
    fn of<F: ?Sized + 'static>(function: &::std::rc::Rc<F>) -> Self { Self(Some((::std::rc::Rc::as_ptr(function).cast::<()>() as usize, Box::new(::std::rc::Rc::downgrade(function))))) }
    /// The token of a borrowed function, keyed by its address alone (no handle to pin).
    fn of_borrowed<F: ?Sized>(function: &F) -> Self { Self(Some((::std::ptr::from_ref(function).cast::<()>() as usize, Box::new(::std::rc::Weak::<()>::new())))) }
    /// The identity key of the function, or `None` for a record no function built.
    fn key(&self) -> Option<usize> { self.0.as_ref().map(|(key, _)| *key) }
}
impl Clone for SmeltFunctionToken { fn clone(&self) -> Self { Self(self.0.as_ref().map(|(key, allocation)| (*key, allocation.clone_box()))) } }
impl ::std::fmt::Debug for SmeltFunctionToken { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("[Function]") } }
impl PartialEq for SmeltFunctionToken { fn eq(&self, _other: &Self) -> bool { true } }
// @smelt:item-end
