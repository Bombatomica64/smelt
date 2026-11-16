// Field guards of widened class handles.
//
// TypeScript lets a generic class instance flow to an instantiation with other
// type arguments (`const wide: Parser<unknown> = new Parser<number>()`). The
// object stays one object: a write through `wide` is seen through the
// original, `wide === original`, and a method called through `wide` runs on
// the original's state. The generated handle of such a class
// (`emitter::widened_view`) is a trait object over the shared cell, and a
// handle whose type arguments differ from the cell's storage holds a
// converting VIEW of that cell instead of a copy.
//
// A view has no `Inner<V>` record to borrow, so field access through a handle
// that may be a view goes through per-field accessors answering these guards:
// on a real cell they are the plain `Ref`/`RefMut` projections, on a view they
// hold the field converted to the handle's arguments, and a write guard puts
// its value back into the source cell when it is dropped (a list field is
// spliced into the source's own array, so the array keeps its identity).

// @smelt:item SmeltFieldRead when widened_views
/// A field read through a handle that may be a converting view.
#[allow(dead_code)]
enum SmeltFieldRead<'a, T> {
    //~ The field of a real cell, borrowed in place.
    Cell(::std::cell::Ref<'a, T>),
    //~ The field of a view's source, converted to the handle's type arguments.
    Owned(T),
}
impl<T> ::std::ops::Deref for SmeltFieldRead<'_, T> { type Target = T; fn deref(&self) -> &T { match self { Self::Cell(guard) => guard, Self::Owned(value) => value } } }
// @smelt:item-end

// @smelt:item SmeltFieldWrite when widened_views
/// A field written through a handle that may be a converting view.
//~
//~ The converted variant writes its value back into the view's source when it
//~ is dropped, which is the end of the statement that took it: one guard per
//~ statement, like the `RefMut` a real cell answers.
#[allow(dead_code)]
enum SmeltFieldWrite<'a, T> {
    //~ The field of a real cell, borrowed mutably in place.
    Cell(::std::cell::RefMut<'a, T>),
    //~ The field of a view's source, converted, with the write-back that stores
    //~ it again at the source's type arguments.
    //~ The value is `Some` until the drop hands it to the write-back.
    Converted(Option<T>, Option<Box<dyn FnOnce(T) + 'a>>),
}
#[allow(dead_code)]
impl<'a, T> SmeltFieldWrite<'a, T> {
    //~ A converted field that `write_back` stores into the view's source.
    fn converted(value: T, write_back: impl FnOnce(T) + 'a) -> Self { Self::Converted(Some(value), Some(Box::new(write_back))) }
}
impl<T> ::std::ops::Deref for SmeltFieldWrite<'_, T> { type Target = T; fn deref(&self) -> &T { match self { Self::Cell(guard) => guard, Self::Converted(value, _) => value.as_ref().expect("a field guard is read before its drop") } } }
impl<T> ::std::ops::DerefMut for SmeltFieldWrite<'_, T> { fn deref_mut(&mut self) -> &mut T { match self { Self::Cell(guard) => guard, Self::Converted(value, _) => value.as_mut().expect("a field guard is written before its drop") } } }
impl<T> Drop for SmeltFieldWrite<'_, T> { fn drop(&mut self) { if let Self::Converted(value, write_back) = self && let (Some(value), Some(write_back)) = (value.take(), write_back.take()) { write_back(value); } } }
// @smelt:item-end

// @smelt:item smelt_view_store when converted_views & unknown
/// Store a value written through a view at a wider type than the object's
/// field has (`{ n: number }` seen as `{ n: number | string }`, written a
/// string): the field keeps its declared type, and the value is converted to
/// it through the erased carrier's checked extraction, as a write through a
/// widened class handle converts.
#[allow(dead_code)]
fn smelt_view_store<T: SmeltFromUnknown>(value: SmeltUnknown) -> T { T::smelt_from_unknown(value) }
// @smelt:item-end

// @smelt:item smelt_widened_view_storage when widened_views
/// An access a placeholder for an object seen at other type arguments cannot
/// answer: a callable slot over the class's parameters that takes its
/// arguments by reference, an upcast or a conversion into an instantiation no
/// view is built at. Reported instead of answering from the placeholder's
/// empty record.
#[allow(dead_code)]
fn smelt_widened_view_storage(class: &str) -> ! { panic!("a `{class}` seen at other type arguments than it was built with cannot answer this access through its converting view") }
// @smelt:item-end

// @smelt:item smelt_widened_cold when widened_views
/// Run a method's forward to a converting view out of line, so the method's
/// direct path stays small enough to inline: a real cell only tests the
/// `smelt_is_view` flag.
#[cold]
#[inline(never)]
#[allow(dead_code)]
fn smelt_widened_cold<R>(forward: impl FnOnce() -> R) -> R { forward() }
// @smelt:item-end
