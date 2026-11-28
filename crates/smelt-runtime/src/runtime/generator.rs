// Generator and async generator state machines.

// Synchronous TypeScript generators are true resumable computations.
// The producer future itself has an anonymous type, so the public ABI
// stores only a typed resume closure; yielded and returned values remain
// concrete and never cross the dynamic `SmeltUnknown` boundary.
// @smelt:item SmeltGeneratorResult when iterator_result
#[derive(Clone, Debug)]
pub enum SmeltGeneratorResult<Y, R> { Yielded(Y), Complete(R) }
//~ `Throw` injects an exception at the suspended `yield`: the error channel's
//~ own `Box<dyn Error>`, which carries whatever the source language throws
//~ (JavaScript's thrown value, wrapped by `smelt_throw`; Python's
//~ `SmeltPyException`), so a `catch`/`except` around the `yield` sees it
//~ exactly as it sees a throw from a call.
//~ `Next` and `Return` say whether the resume carried a value: `gen.next()`
//~ and `for...of` send `Next(None)`, `gen.next(v)` sends `Next(Some(v))`. A
//~ generator body reads `None` as its resume type's default (as before); a
//~ hand-written iterator's `next(sent?)` sees `undefined`
//~ (`SmeltGenerator::from_protocol`), and `yield*` forwards it unchanged.
#[derive(Debug)]
pub enum SmeltGeneratorCommand<N, R> { Next(Option<N>), Return(Option<R>), Throw(Box<dyn ::std::error::Error>) }
//~ A hand-written iterator's protocol method called with no argument
//~ (`next()`, `return()`), and one called with the value sent.
type SmeltProtocolCall<Y, R> = ::std::rc::Rc<dyn Fn() -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>>>;
type SmeltProtocolSend<A, Y, R> = ::std::rc::Rc<dyn Fn(A) -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>>>;
pub struct SmeltGenerator<Y, R, N> {
    resume: ::std::rc::Rc<::std::cell::RefCell<Box<dyn FnMut(SmeltGeneratorCommand<N, R>) -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>>>>>,
    completed: ::std::rc::Rc<::std::cell::RefCell<Option<R>>>,
    //~ Whether the body has been entered (a first `next`/`send` ran it).
    started: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ Whether this is a generator body, finished for good once completed and
    //~ not yet entered before its first resume. A hand-written iterator's
    //~ carrier (`from_protocol`) is neither: its methods run on every call,
    //~ as JavaScript calls them.
    latches: bool,
    //~ The object a hand-written iterator carrier adapts (`return this`), so
    //~ `===` sees the adapter as that object (`it[Symbol.iterator]() === it`).
    source: Option<::std::rc::Rc<dyn ::std::any::Any>>,
}
impl<Y, R, N> Clone for SmeltGenerator<Y, R, N> { fn clone(&self) -> Self { Self { resume: self.resume.clone(), completed: self.completed.clone(), started: self.started.clone(), latches: self.latches, source: self.source.clone() } } }
//~ A generator is an object: two are `===` exactly when they are the same
//~ one (`it[Symbol.iterator]() === it`), which is sharing one state machine,
//~ or adapting the same hand-written iterator object.
impl<Y, R, N> PartialEq for SmeltGenerator<Y, R, N> { fn eq(&self, other: &Self) -> bool { self.identity() == other.identity() } }
impl<Y, R, N> SmeltGenerator<Y, R, N> {
    //~ The JavaScript identity of the carrier: the address of the object a
    //~ hand-written iterator carrier adapts (the cell a reference class's
    //~ handle shares, so `===` against that object compares addresses), else
    //~ of its own state machine.
    #[allow(dead_code)]
    fn identity(&self) -> usize { self.source.as_ref().map_or_else(|| ::std::rc::Rc::as_ptr(&self.resume).cast::<()>() as usize, |source| ::std::rc::Rc::as_ptr(source).cast::<()>() as usize) }
}
impl<Y, R, N> ::std::fmt::Debug for SmeltGenerator<Y, R, N> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("SmeltGenerator") } }
impl<Y, R: Clone + Default, N> SmeltGenerator<Y, R, N> {
    //~ Hide the producer future type while retaining concrete yield/return types.
    fn new(resume: impl FnMut(SmeltGeneratorCommand<N, R>) -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>> + 'static) -> Self { Self { resume: ::std::rc::Rc::new(::std::cell::RefCell::new(Box::new(resume))), completed: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), started: ::std::rc::Rc::new(::std::cell::Cell::new(false)), latches: true, source: None } }
    //~ Resume execution until the next suspension point or completion, or
    //~ until the body throws: the throw is returned, so a `try` around
    //~ `.next()` catches it, and the generator is done from then on (a later
    //~ `next()` answers `{ value: undefined, done: true }`).
    //~
    //~ A `Return` or `Throw` sent before the body was ever entered completes
    //~ the generator without running it (ECMAScript's `GeneratorResumeAbrupt`
    //~ in state suspended-start; CPython's `gen.throw`/`gen.close` on a
    //~ just-started generator): the return value is answered, the throw raised.
    fn try_resume(&self, command: SmeltGeneratorCommand<N, R>) -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>> {
        if self.latches {
            if self.completed.borrow().is_some() { return Ok(SmeltGeneratorResult::Complete(R::default())); }
            if !self.started.replace(true) {
                match command {
                    SmeltGeneratorCommand::Next(_) => {}
                    SmeltGeneratorCommand::Return(value) => { let value = value.unwrap_or_default(); *self.completed.borrow_mut() = Some(value.clone()); return Ok(SmeltGeneratorResult::Complete(value)); }
                    SmeltGeneratorCommand::Throw(error) => { *self.completed.borrow_mut() = Some(R::default()); return Err(error); }
                }
            }
        }
        let result = (self.resume.borrow_mut())(command);
        match &result {
            Ok(SmeltGeneratorResult::Complete(value)) => *self.completed.borrow_mut() = Some(value.clone()),
            Err(_) => *self.completed.borrow_mut() = Some(R::default()),
            Ok(SmeltGeneratorResult::Yielded(_)) => {}
        }
        result
    }
    //~ [`Self::try_resume`] for a consumer with no `try` to deliver a throw to:
    //~ the throw unwinds as an uncaught one.
    fn resume(&self, command: SmeltGeneratorCommand<N, R>) -> SmeltGeneratorResult<Y, R> { self.try_resume(command).unwrap_or_else(|error| smelt_panic_throw(error)) }
    //~ Whether the body has been entered: a generator neither resumed yet nor
    //~ completed is just-started.
    #[allow(dead_code)]
    fn is_started(&self) -> bool { self.started.get() }
}
impl<Y: 'static, R: Clone + Default + 'static, N: 'static> SmeltGenerator<Y, R, N> {
    //~ A hand-written iterator object as the carrier: each command calls the
    //~ iterator's own protocol method, as JavaScript's consumers do.
    //~
    //~ * `Next(None)` calls `next()`, `Next(Some(v))` calls `next(v)`
    //~   (`next_with`; a `next` that declares no parameter ignores `v`).
    //~ * `Return(..)` calls `return()` / `return(v)` and answers its step,
    //~   which may even be a `Yielded` one; without `return` the carrier
    //~   completes with the value it was sent.
    //~ * `Throw(e)` calls `throw(e)` (with the thrown value the error box
    //~   carries); without `throw` it closes the iterator
    //~   (its `return()`) and throws the `TypeError` `yield*` throws then
    //~   (ECMA-262 `yield*`, step "throw is undefined").
    //~
    //~ The carrier does not latch: a `next()` after a `done` step calls the
    //~ iterator again. `source` is the adapted object (identity, see `PartialEq`).
    #[allow(dead_code, clippy::too_many_arguments)]
    fn from_protocol(source: Option<::std::rc::Rc<dyn ::std::any::Any>>, next: SmeltProtocolCall<Y, R>, next_with: Option<SmeltProtocolSend<N, Y, R>>, close: Option<SmeltProtocolCall<Y, R>>, close_with: Option<SmeltProtocolSend<R, Y, R>>, throw_with: Option<SmeltProtocolSend<Box<dyn ::std::error::Error>, Y, R>>) -> Self {
        let resume = move |command: SmeltGeneratorCommand<N, R>| match command {
            SmeltGeneratorCommand::Next(Some(sent)) => next_with.as_ref().map_or_else(|| next(), |next_with| next_with(sent)),
            SmeltGeneratorCommand::Next(None) => next(),
            SmeltGeneratorCommand::Return(Some(value)) => match (&close_with, &close) { (Some(close_with), _) => close_with(value), (None, Some(close)) => close(), (None, None) => Ok(SmeltGeneratorResult::Complete(value)) },
            SmeltGeneratorCommand::Return(None) => close.as_ref().map_or_else(|| Ok(SmeltGeneratorResult::Complete(R::default())), |close| close()),
            SmeltGeneratorCommand::Throw(error) => if let Some(throw_with) = &throw_with { throw_with(error) } else {
                if let Some(close) = &close { close()?; }
                Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TypeError".into())), ("message".to_owned(), SmeltUnknown::String("The iterator does not provide a 'throw' method.".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))))
            },
        };
        Self { resume: ::std::rc::Rc::new(::std::cell::RefCell::new(Box::new(resume))), completed: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), started: ::std::rc::Rc::new(::std::cell::Cell::new(true)), latches: false, source }
    }
}
//~ A collection iterator (`map.keys()`, `set.values()`, `map.entries()`):
//~ single-pass over the collection's live store, reading position `index`
//~ through `at` until it answers `None`. It is a generator, so it has
//~ `next()`, is consumed by `for...of`/spread/`Array.from`, and is its own
//~ iterator, as JavaScript's is.
fn smelt_live_iterator<Y: 'static>(mut at: impl FnMut(usize) -> Option<Y> + 'static) -> SmeltGenerator<Y, (), ()> {
    let mut index = 0_usize;
    SmeltGenerator::new(move |_command| Ok(at(index).map_or(SmeltGeneratorResult::Complete(()), |value| { index = index.saturating_add(1); SmeltGeneratorResult::Yielded(value) })))
}
impl<Y, R: Clone + Default, N: Default> SmeltGenerator<Y, R, N> {
    //~ Drain the generator's remaining yields in order (`Array.from(gen)` / `[...gen]`).
    #[allow(dead_code)]
    fn collect_yields(&self) -> Vec<Y> { let mut items = Vec::new(); while let SmeltGeneratorResult::Yielded(value) = self.resume(SmeltGeneratorCommand::Next(None)) { items.push(value); } items }
    //~ [`Self::collect_yields`] that returns a throw from the body instead of
    //~ unwinding, so a `try` around `[...gen]` catches it.
    //~ At most `count` yields, then close the generator (`IteratorClose`:
    //~ resume it with `Return`, which runs its pending `finally` blocks or a
    //~ hand-written iterator's `return()`), as array destructuring without a
    //~ rest element does (`const [a, b] = it`). One that completed first is
    //~ not closed. A throw from the body or the close is returned.
    #[allow(dead_code)]
    fn try_take_yields(&self, count: usize) -> Result<Vec<Y>, Box<dyn ::std::error::Error>> { let mut items = Vec::new(); while items.len() < count { match self.try_resume(SmeltGeneratorCommand::Next(None))? { SmeltGeneratorResult::Yielded(value) => items.push(value), SmeltGeneratorResult::Complete(_) => return Ok(items) } } self.try_resume(SmeltGeneratorCommand::Return(None))?; Ok(items) }
    #[allow(dead_code)]
    fn try_collect_yields(&self) -> Result<Vec<Y>, Box<dyn ::std::error::Error>> { let mut items = Vec::new(); while let SmeltGeneratorResult::Yielded(value) = self.try_resume(SmeltGeneratorCommand::Next(None))? { items.push(value); } Ok(items) }
}
pub struct SmeltAsyncGenerator<Y, R, N> {
    resume: ::std::rc::Rc<dyn Fn(SmeltGeneratorCommand<N, R>) -> SmeltFuture<SmeltGeneratorResult<Y, R>>>,
    completed: ::std::rc::Rc<::std::cell::RefCell<Option<R>>>,
}
impl<Y, R, N> Clone for SmeltAsyncGenerator<Y, R, N> { fn clone(&self) -> Self { Self { resume: self.resume.clone(), completed: self.completed.clone() } } }
impl<Y, R, N> ::std::fmt::Debug for SmeltAsyncGenerator<Y, R, N> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("SmeltAsyncGenerator") } }
impl<Y: Clone + 'static, R: Clone + 'static, N> SmeltAsyncGenerator<Y, R, N> {
    fn new(resume: impl Fn(SmeltGeneratorCommand<N, R>) -> SmeltFuture<SmeltGeneratorResult<Y, R>> + 'static) -> Self { Self { resume: ::std::rc::Rc::new(resume), completed: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }
    fn resume(&self, command: SmeltGeneratorCommand<N, R>) -> SmeltFuture<SmeltGeneratorResult<Y, R>> { if let Some(value) = self.completed.borrow().clone() { return SmeltFuture::from_future(Box::pin(async move { Ok(SmeltGeneratorResult::Complete(value)) })); } let future = (self.resume)(command); let completed = self.completed.clone(); SmeltFuture::from_future(Box::pin(async move { let result = future.await?; if let SmeltGeneratorResult::Complete(value) = &result { *completed.borrow_mut() = Some(value.clone()); } Ok(result) })) }
}
//~ Genuine dynamic boundary: a generator crossing into source `unknown`
//~ (e.g. a `function*` value stored in an erased callable slot, or a
//~ generator object handed to an `unknown`-typed parameter) is a live
//~ resumable state machine. No concrete type, generated union, or
//~ scoped generic can represent it on the erased side — the receiver
//~ only knows the JavaScript iterator protocol. The adapter therefore
//~ reproduces exactly that protocol: an object with a callable `next`
//~ that resumes the same shared state machine and returns erased
//~ `{ value, done }` steps, plus a `__smelt_generator` marker for tag
//~ checks. Yielded/returned values erase through `IntoSmeltUnknown` at
//~ the step boundary; resume inputs are dropped because the erased
//~ protocol cannot type them (matching a bare `next()` call).
impl<Y: IntoSmeltUnknown + 'static, R: IntoSmeltUnknown + Clone + Default + 'static, N: Default + 'static> IntoSmeltUnknown for SmeltGenerator<Y, R, N> {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let generator = self;
        //~ Its marker and `next` are not own enumerable properties (they live on
        //~ the iterator prototype in JavaScript), so they sit under the hidden
        //~ `__smelt_method:` prefix: `JSON.stringify(map.keys())` is `{}` and
        //~ `Object.keys` of it is empty. `[Symbol.iterator]()` answers the
        //~ iterator itself, through a weak handle so the object is no cycle.
        let object = SmeltObject::new(Vec::from([("__smelt_method:__smelt_generator".to_owned(), SmeltUnknown::Bool(true))]));
        object.insert("__smelt_method:next", SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let mut step: Vec<(String, SmeltUnknown)> = Vec::new(); match generator.try_resume(SmeltGeneratorCommand::Next(None))? { SmeltGeneratorResult::Yielded(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(false))); } SmeltGeneratorResult::Complete(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(true))); } } Ok(SmeltUnknown::Object(SmeltObject::new(step))) })));
        let itself = object.downgrade();
        object.insert("__smelt_symbol_iterator", SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(itself.upgrade().map_or(SmeltUnknown::Undefined, SmeltUnknown::Object)))));
        SmeltUnknown::Object(object)
    }
}
//~ Async flavor of the same boundary: `next` returns an erased promise
//~ that resolves to the `{ value, done }` step, mirroring the async
//~ iterator protocol an erased consumer would drive, and
//~ `[Symbol.asyncIterator]()` answers the iterator itself through a weak
//~ handle, as the synchronous one does.
impl<Y: IntoSmeltUnknown + Clone + 'static, R: IntoSmeltUnknown + Clone + 'static, N: Default + 'static> IntoSmeltUnknown for SmeltAsyncGenerator<Y, R, N> {
    fn into_smelt_unknown(self) -> SmeltUnknown { let generator = self; let mut object: Vec<(String, SmeltUnknown)> = Vec::new(); object.push(("__smelt_generator".to_owned(), SmeltUnknown::Bool(true))); let next: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let future = generator.resume(SmeltGeneratorCommand::Next(None)); Ok(SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move { let mut step: Vec<(String, SmeltUnknown)> = Vec::new(); match future.await? { SmeltGeneratorResult::Yielded(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(false))); } SmeltGeneratorResult::Complete(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(true))); } } Ok(SmeltUnknown::Object(SmeltObject::new(step))) })))) }); object.push(("next".to_owned(), SmeltUnknown::Function(next.clone()))); let object = SmeltObject::new(object); let itself = object.downgrade(); object.insert("__smelt_symbol_async_iterator", SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(itself.upgrade().map_or(SmeltUnknown::Undefined, SmeltUnknown::Object))))); SmeltUnknown::Object(object) }
}

// @smelt:item-end
