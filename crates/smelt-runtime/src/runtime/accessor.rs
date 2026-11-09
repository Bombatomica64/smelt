// Record fields stored as accessors (object-literal `get`ters).
//
// JavaScript evaluates a getter on every read:
//
// ```ts
// function makeCounter(): Counter {
//   let value: number | undefined;
//   return { bump: () => { value = (value ?? 0) + 1; }, get cachedValue() { return value; } };
// }
// ```
//
// `counter.cachedValue` answers `undefined` before `bump()` and `1` after it.
// A struct field holding the getter's value at construction froze it at the
// first value. A hand port stores the getter and calls it on each read, so a
// record field that some construction site supplies through a getter
// (`smelt_hir::Crate::accessor_fields`, decided by the frontend) is stored as
// a `SmeltAccessor<T>` ([`ACCESSOR_TYPE`]) instead of a `T`:
//
// * construction from the literal hands the getter closure to
//   `SmeltAccessor::new`; any other construction (an adapter rebuilding the
//   record, a `Default`) supplies a plain value through
//   `SmeltAccessor::constant`;
// * every read of the field is `.get()`, which runs the getter now —
//   a direct read, a destructuring, a spread (which copies the current value,
//   as JavaScript's spread does) and the erasure into `SmeltUnknown` (the
//   getter is evaluated at erase time).
//
// Records no literal supplies through a getter keep plain fields.

// Emits the accessor storage type.
//
// With the erased runtime present (the `unknown` gate), an accessor also
// crosses the `SmeltUnknown` boundary: erasing it reads the getter at that
// point, and recovering one from an erased value answers that value on every
// read (the erased object carries no live getter for a record field).
// @smelt:item impl_IntoSmeltUnknown_for_SmeltAccessor_T when accessors & unknown
impl<T: IntoSmeltUnknown + 'static> IntoSmeltUnknown for SmeltAccessor<T> { fn into_smelt_unknown(self) -> SmeltUnknown { self.get().into_smelt_unknown() } }
impl<T: SmeltFromUnknown + Clone + 'static> SmeltFromUnknown for SmeltAccessor<T> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { Self::constant(T::smelt_from_unknown(value)) } }
// @smelt:item-end

// @smelt:item SmeltAccessor when accessors
/// A record field stored as a getter: every read runs it.
struct SmeltAccessor<T>(::std::rc::Rc<dyn Fn() -> T>);
impl<T: 'static> SmeltAccessor<T> { fn new(getter: ::std::rc::Rc<dyn Fn() -> T>) -> Self { Self(getter) } fn get(&self) -> T { (self.0)() } }
impl<T: Clone + 'static> SmeltAccessor<T> { fn constant(value: T) -> Self { Self(::std::rc::Rc::new(move || value.clone())) } }
impl<T> Clone for SmeltAccessor<T> { fn clone(&self) -> Self { Self(self.0.clone()) } }
impl<T: Default + Clone + 'static> Default for SmeltAccessor<T> { fn default() -> Self { Self::constant(T::default()) } }
impl<T: ::std::fmt::Debug + 'static> ::std::fmt::Debug for SmeltAccessor<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.get().fmt(formatter) } }
impl<T: PartialEq + 'static> PartialEq for SmeltAccessor<T> { fn eq(&self, other: &Self) -> bool { self.get() == other.get() } }
// @smelt:item-end
