// Generator and async generator state machines.

// Synchronous TypeScript generators are true resumable computations.
// The producer future itself has an anonymous type, so the public ABI
// stores only a typed resume closure; yielded and returned values remain
// concrete and never cross the dynamic `SmeltUnknown` boundary.
// @smelt:item SmeltGeneratorResult when iterator_result
#[derive(Clone, Debug)]
pub enum SmeltGeneratorResult<Y, R> { Yielded(Y), Complete(R) }
//~ JavaScript permits throwing any runtime value, independently of the
//~ generator's concrete Y/R/N parameters. This is a genuine dynamic
//~ boundary: preserving a number, object, symbol, or string for a catch
//~ binding requires the tagged runtime carrier rather than a concrete
//~ Rust generic shared with the other protocol channels.
#[derive(Clone, Debug)]
pub enum SmeltGeneratorCommand<N, R> { Next(N), Return(R), Throw(SmeltUnknown) }
pub struct SmeltGenerator<Y, R, N> {
    resume: ::std::rc::Rc<::std::cell::RefCell<Box<dyn FnMut(SmeltGeneratorCommand<N, R>) -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>>>>>,
    completed: ::std::rc::Rc<::std::cell::RefCell<Option<R>>>,
}
impl<Y, R, N> Clone for SmeltGenerator<Y, R, N> { fn clone(&self) -> Self { Self { resume: self.resume.clone(), completed: self.completed.clone() } } }
impl<Y, R, N> ::std::fmt::Debug for SmeltGenerator<Y, R, N> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("SmeltGenerator") } }
impl<Y, R: Clone + Default, N> SmeltGenerator<Y, R, N> {
    //~ Hide the producer future type while retaining concrete yield/return types.
    fn new(resume: impl FnMut(SmeltGeneratorCommand<N, R>) -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>> + 'static) -> Self { Self { resume: ::std::rc::Rc::new(::std::cell::RefCell::new(Box::new(resume))), completed: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }
    //~ Resume execution until the next suspension point or completion, or
    //~ until the body throws: the throw is returned, so a `try` around
    //~ `.next()` catches it, and the generator is done from then on (a later
    //~ `next()` answers `{ value: undefined, done: true }`).
    fn try_resume(&self, command: SmeltGeneratorCommand<N, R>) -> Result<SmeltGeneratorResult<Y, R>, Box<dyn ::std::error::Error>> {
        if let Some(value) = self.completed.borrow().clone() { return Ok(SmeltGeneratorResult::Complete(value)); }
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
}
impl<Y, R: Clone + Default, N: Default> SmeltGenerator<Y, R, N> {
    //~ Drain the generator's remaining yields in order (`Array.from(gen)` / `[...gen]`).
    #[allow(dead_code)]
    fn collect_yields(&self) -> Vec<Y> { let mut items = Vec::new(); while let SmeltGeneratorResult::Yielded(value) = self.resume(SmeltGeneratorCommand::Next(N::default())) { items.push(value); } items }
    //~ [`Self::collect_yields`] that returns a throw from the body instead of
    //~ unwinding, so a `try` around `[...gen]` catches it.
    #[allow(dead_code)]
    fn try_collect_yields(&self) -> Result<Vec<Y>, Box<dyn ::std::error::Error>> { let mut items = Vec::new(); while let SmeltGeneratorResult::Yielded(value) = self.try_resume(SmeltGeneratorCommand::Next(N::default()))? { items.push(value); } Ok(items) }
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
    fn into_smelt_unknown(self) -> SmeltUnknown { let generator = self; let mut object: Vec<(String, SmeltUnknown)> = Vec::new(); object.push(("__smelt_generator".to_owned(), SmeltUnknown::Bool(true))); object.push(("next".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let mut step: Vec<(String, SmeltUnknown)> = Vec::new(); match generator.try_resume(SmeltGeneratorCommand::Next(Default::default()))? { SmeltGeneratorResult::Yielded(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(false))); } SmeltGeneratorResult::Complete(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(true))); } } Ok(SmeltUnknown::Object(SmeltObject::new(step))) })))); SmeltUnknown::Object(SmeltObject::new(object)) }
}
//~ Async flavor of the same boundary: `next` returns an erased promise
//~ that resolves to the `{ value, done }` step, mirroring the async
//~ iterator protocol an erased consumer would drive.
impl<Y: IntoSmeltUnknown + Clone + 'static, R: IntoSmeltUnknown + Clone + 'static, N: Default + 'static> IntoSmeltUnknown for SmeltAsyncGenerator<Y, R, N> {
    fn into_smelt_unknown(self) -> SmeltUnknown { let generator = self; let mut object: Vec<(String, SmeltUnknown)> = Vec::new(); object.push(("__smelt_generator".to_owned(), SmeltUnknown::Bool(true))); object.push(("next".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let future = generator.resume(SmeltGeneratorCommand::Next(Default::default())); Ok(SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move { let mut step: Vec<(String, SmeltUnknown)> = Vec::new(); match future.await? { SmeltGeneratorResult::Yielded(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(false))); } SmeltGeneratorResult::Complete(value) => { step.push(("value".to_owned(), value.into_smelt_unknown())); step.push(("done".to_owned(), SmeltUnknown::Bool(true))); } } Ok(SmeltUnknown::Object(SmeltObject::new(step))) })))) })))); SmeltUnknown::Object(SmeltObject::new(object)) }
}

// @smelt:item-end
