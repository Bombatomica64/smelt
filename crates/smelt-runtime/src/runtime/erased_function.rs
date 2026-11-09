// `SmeltErasedFunction`: the callable that crosses the erased ABI.

// @smelt:item SmeltErasedFunction when erased_function
#[derive(Clone)]
pub struct SmeltErasedFunction {
    callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> SmeltUnknown>,
    length: f64,
    object: Option<SmeltObject>,
}

//~ The placeholder an erased callable takes before the real one is
//~ built (a predeclared function-typed local, a callable-interface
//~ `__smelt_call` slot, or the self-recursive closure knot, whose
//~ cell exists before the closure that fills it). It is runtime
//~ machinery rather than program data, so it lives here instead of
//~ being inlined at every emit site. See `ERASED_FUNCTION_DEFAULT`.
impl Default for SmeltErasedFunction {
    fn default() -> Self {
        Self { callback: ::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| SmeltUnknown::Null), length: 0.0, object: None }
    }
}

impl SmeltErasedFunction {
    //~ Invoke an erased JavaScript callable through a reentrant handle.
    fn call(&self, args: impl Into<Vec<SmeltUnknown>>) -> SmeltUnknown {
        (self.callback)(args.into())
    }
    //~ A callable object narrowed to a callable interface that declares
    //~ fewer members keeps its dropped members as own properties of the
    //~ underlying function value, exactly as JavaScript does (es-toolkit's
    //~ `curry` assigns `wrapper.placeholder`, and its `flow` spec reads
    //~ that property back through a `CurriedFunction1`). They are parked
    //~ in the identity-keyed function property bag rather than in
    //~ the `object` bag: `object` is what makes an erased callable erase
    //~ as an OBJECT carrying `__smelt_call`, and the value being narrowed
    //~ here is still a plain function everywhere else, so filling `object`
    //~ would double-wrap it at the next erasure.
    //~ Record a callable object's own JavaScript properties.
    fn smelt_with_properties(self, entries: Vec<(String, SmeltUnknown)>) -> Self {
        for (name, value) in entries { smelt_set_function_property(&self.callback, &name, value); }
        self
    }

    //~ A property write onto a function value (`partialed.prototype = ..`)
    //~ has to land somewhere the value's OTHER representations can see,
    //~ and the identity-keyed bag is that place -- writing into `object`
    //~ would turn a plain function into a callable OBJECT at its next
    //~ erasure, which is a different JavaScript value.
    //~ Store one own JavaScript property on this function value.
    fn smelt_set_property(&self, name: &str, value: SmeltUnknown) { smelt_set_function_property(&self.callback, name, value); }
// @smelt:if this_channel
    //~ Binding a receiver keeps the callable TYPED: a bound method is
    //~ still a function everywhere else in the program, so degrading it
    //~ to `SmeltUnknown` at the bind would erase a shape the source
    //~ states. `object` rides along so a bound callable object keeps its
    //~ own properties.
    //~ Bind a receiver to this callable, as `Function.prototype.bind` does.
    fn smelt_bind_this(&self, receiver: SmeltUnknown) -> SmeltErasedFunction {
        let callback = self.callback.clone();
        SmeltErasedFunction {
            callback: ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
                let _smelt_this_guard = smelt_push_this(receiver.clone());
                (callback)(args)
            }),
            length: self.length,
            object: self.object.clone(),
        }
    }
// @smelt:end
    //~ Read one own property of a callable object, `undefined` when absent.
    //~
    //~ `prototype` is materialised on first read exactly as it is for an
    //~ erased function value, so a still-typed callable answers the same
    //~ object the erased one does.
    fn smelt_property(&self, name: &str) -> SmeltUnknown {
        if let Some(value) = self.object.as_ref().and_then(|object| object.get(name)) { return value; }
        let identity = smelt_canonical_function_identity(&self.callback);
        if let Some(value) = smelt_function_property_lookup(&identity, name) { return value; }
        if name == "prototype" { return smelt_function_prototype(&identity, self.clone().into_smelt_unknown()); }
        SmeltUnknown::Undefined
    }
    //~ Restore an erased callable value without dropping callable-object fields.
    fn into_smelt_unknown(self) -> SmeltUnknown {
        //~ Erasing the SAME callback twice must yield `SmeltUnknown::Function`
        //~ values that share one OUTER `Rc`, because reference identity
        //~ (`Rc::ptr_eq` in `same_js_key`/`smelt_unknown_structural_eq`) compares
        //~ that outer `Rc`. A nullary function-item constant (`doNothing()`)
        //~ routes through one cached `SmeltErasedFunction`, so two calls share
        //~ the inner callback `Rc`; keep a `Weak` to the outer value in its entry so
        //~ both erasures resolve to one `SmeltUnknown::Function` while both are
        //~ alive (e.g. inside one `toStrictEqual`). A `Weak` avoids pinning the
        //~ callback alive; a successful upgrade proves the address is still the
        //~ same callback, so it is a true hit. Callable objects
        //~ (`object: Some(_)`) are per-instance and skip the cache.
        if self.object.is_none() {
            if let Some(callable) = smelt_read_callable(&self.callback, |meta| meta.erased_value.as_ref().and_then(::std::rc::Weak::upgrade)) {
                return SmeltUnknown::Function(callable);
            }
            let callback = self.callback.clone();
            let callable: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((callback)(args)));
            let erased_value = ::std::rc::Rc::downgrade(&callable);
            smelt_update_callable(&self.callback, |meta| meta.erased_value = Some(erased_value));
            //~ The `length` field dies with the struct, so hand it to the registry
            //~ before returning: this is the only point that still knows both the
            //~ arity and the erased allocation it belongs to.
            //~ The erased value and the typed callable are ONE JavaScript
            //~ function, so they must resolve to one canonical identity: that is
            //~ what makes an own property written through either spelling
            //~ (`f.prototype`) readable through the other.
            smelt_link_function_identity(&callable, &self.callback);
            smelt_register_function_length_once(&callable, self.length);
            return SmeltUnknown::Function(callable);
        }
        let callback = self.callback.clone();
        let length = self.length;
        let callable = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((callback)(args)));
        smelt_link_function_identity(&callable, &self.callback);
        smelt_register_function_length_once(&callable, length);
        let callable = SmeltUnknown::Function(callable);
        if let Some(object) = self.object { object.insert("__smelt_call".to_owned(), callable); SmeltUnknown::Object(object) } else { callable }
    }
}

// @smelt:item-end
