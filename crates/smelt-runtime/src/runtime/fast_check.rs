// The `fast-check` property-testing library.
//
// **Dynamic boundary.** `fast-check` is an untyped package to the transpiler
// (`untyped_imports`), so the values it generates reach the property callback
// as `SmeltUnknown`, and an arbitrary is a value whose shape only the builder
// calls that made it know. A program's own types are untouched.
//
// Generation is deterministic: a fixed seed unless the source passes `seed`, so
// a failing generated test fails the same way on every run. Unlike the real
// library this model does not shrink a counterexample; shrinking changes the
// message, never whether the property holds.

// @smelt:item smelt_fast_check_namespace when fast_check
thread_local! { static SMELT_FC_STATE: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0x9E37_79B9_7F4A_7C15) }; }

/// The next 64 random bits (xorshift64*).
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_next() -> u64 {
    SMELT_FC_STATE.with(|state| {
        let mut x = state.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        state.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

/// A uniform integer in `low..=high`.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_int(low: i64, high: i64) -> i64 {
    if high <= low { return low; }
    let span = (high as i128 - low as i128 + 1) as u128;
    (low as i128 + (smelt_fc_next() as u128 % span) as i128) as i64
}

/// A numeric field of an options record.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_option(options: Option<&SmeltUnknown>, key: &str) -> Option<f64> {
    match options { Some(SmeltUnknown::Object(object)) => match object.get(key) { Some(SmeltUnknown::Number(value)) => Some(value), _ => None }, _ => None }
}

/// A boolean field of an options record.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_flag(options: Option<&SmeltUnknown>, key: &str) -> bool {
    matches!(options, Some(SmeltUnknown::Object(object)) if matches!(object.get(key), Some(SmeltUnknown::Bool(true))))
}

type SmeltFcGenerate = ::std::rc::Rc<dyn Fn() -> Result<SmeltUnknown, Box<dyn std::error::Error>>>;

/// Wrap a generator as an arbitrary value with `map`, `filter` and `chain`.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_arbitrary(generate: SmeltFcGenerate) -> SmeltUnknown {
    let object = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
    let draw = generate.clone();
    object.insert("__smelt_fc_generate".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |_args| draw())));
    let source = generate.clone();
    object.insert("map".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
        let mapper = smelt_fc_callable(args.first())?;
        let source = source.clone();
        Ok(smelt_fc_arbitrary(::std::rc::Rc::new(move || mapper(vec![source()?]))))
    })));
    let source = generate.clone();
    object.insert("filter".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
        let predicate = smelt_fc_callable(args.first())?;
        let source = source.clone();
        Ok(smelt_fc_arbitrary(::std::rc::Rc::new(move || {
            for _ in 0..10_000 {
                let candidate = source()?;
                if smelt_test_truthy(&predicate(vec![candidate.clone()])?) { return Ok(candidate); }
            }
            Err(smelt_throw(SmeltUnknown::String("fast-check: filter rejected every generated value".into())))
        })))
    })));
    let source = generate;
    object.insert("chain".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
        let binder = smelt_fc_callable(args.first())?;
        let source = source.clone();
        Ok(smelt_fc_arbitrary(::std::rc::Rc::new(move || smelt_fc_draw(&binder(vec![source()?])?))))
    })));
    SmeltUnknown::Object(object)
}

/// The callable in `value`, or the error a non-function argument raises.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_callable(value: Option<&SmeltUnknown>) -> Result<SmeltDynCallable, Box<dyn std::error::Error>> {
    value.and_then(smelt_callable_of).ok_or_else(|| smelt_throw(SmeltUnknown::String("fast-check: expected a function".into())))
}

/// Generate one value from an arbitrary.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_draw(arbitrary: &SmeltUnknown) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {
    let generate = match arbitrary { SmeltUnknown::Object(object) => object.get("__smelt_fc_generate"), _ => None };
    match generate.as_ref().and_then(smelt_callable_of) {
        Some(generate) => generate(Vec::new()),
        None => Err(smelt_throw(SmeltUnknown::String("fast-check: expected an arbitrary".into()))),
    }
}

/// A length in `minLength..=maxLength`, defaulting to at most ten.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_length(options: Option<&SmeltUnknown>) -> usize {
    let low = smelt_fc_option(options, "minLength").unwrap_or(0.0) as i64;
    let high = smelt_fc_option(options, "maxLength").unwrap_or((low + 10) as f64) as i64;
    smelt_fc_int(low, high) as usize
}

/// Printable ASCII text of `length` characters.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_text(length: usize) -> String {
    (0..length).map(|_| char::from(smelt_fc_int(0x20, 0x7e) as u8)).collect()
}

/// A value of no particular shape, as `fc.anything()` generates.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_anything(depth: usize) -> SmeltUnknown {
    match smelt_fc_int(0, if depth == 0 { 5 } else { 7 }) {
        0 => SmeltUnknown::Null,
        1 => SmeltUnknown::Undefined,
        2 => SmeltUnknown::Bool(smelt_fc_int(0, 1) == 1),
        3 => SmeltUnknown::Number(smelt_fc_int(-1000, 1000) as f64),
        4 => SmeltUnknown::Number(smelt_fc_int(-1000, 1000) as f64 / 8.0),
        5 => SmeltUnknown::String(smelt_fc_text(smelt_fc_int(0, 8) as usize).into()),
        6 => SmeltUnknown::Array(SmeltArray::new((0..smelt_fc_int(0, 4)).map(|_| smelt_fc_anything(depth - 1)).collect())),
        _ => {
            let object = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
            for _ in 0..smelt_fc_int(0, 4) { object.insert(smelt_fc_text(smelt_fc_int(1, 4) as usize), smelt_fc_anything(depth - 1)); }
            SmeltUnknown::Object(object)
        }
    }
}

/// The arbitrary behind a builder named `name`, called with `args`.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_build(name: &'static str, args: Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {
    let options = args.first().cloned();
    let generate: SmeltFcGenerate = match name {
        "integer" | "nat" => {
            let number_form = matches!(options, Some(SmeltUnknown::Number(_)));
            let (default_low, default_high) = if name == "nat" { (0.0, 2147483647.0) } else { (-2147483648.0, 2147483647.0) };
            let low = if number_form { default_low } else { smelt_fc_option(options.as_ref(), "min").unwrap_or(default_low) } as i64;
            let high = match options { Some(SmeltUnknown::Number(max)) => max, _ => smelt_fc_option(options.as_ref(), "max").unwrap_or(default_high) } as i64;
            ::std::rc::Rc::new(move || Ok(SmeltUnknown::Number(match smelt_fc_int(0, 7) { 0 => low, 1 => high, 2 if low <= 0 && high >= 0 => 0, _ => smelt_fc_int(low, high) } as f64)))
        }
        "float" | "double" => {
            let low = smelt_fc_option(options.as_ref(), "min").unwrap_or(-1e9);
            let high = smelt_fc_option(options.as_ref(), "max").unwrap_or(1e9);
            let bounded = smelt_fc_option(options.as_ref(), "min").is_some() || smelt_fc_option(options.as_ref(), "max").is_some();
            let no_nan = smelt_fc_flag(options.as_ref(), "noNaN") || bounded;
            let no_infinity = smelt_fc_flag(options.as_ref(), "noDefaultInfinity") || bounded;
            ::std::rc::Rc::new(move || Ok(SmeltUnknown::Number(match smelt_fc_int(0, 15) {
                0 if !no_nan => f64::NAN,
                1 if !no_infinity => f64::INFINITY,
                2 if !no_infinity => f64::NEG_INFINITY,
                3 => low,
                4 => high,
                _ => low + (smelt_fc_next() as f64 / u64::MAX as f64) * (high - low),
            })))
        }
        "boolean" => ::std::rc::Rc::new(|| Ok(SmeltUnknown::Bool(smelt_fc_int(0, 1) == 1))),
        "string" => ::std::rc::Rc::new(move || Ok(SmeltUnknown::String(smelt_fc_text(smelt_fc_length(options.as_ref())).into()))),
        "anything" => ::std::rc::Rc::new(|| Ok(smelt_fc_anything(2))),
        "constant" => { let value = options.unwrap_or(SmeltUnknown::Undefined); ::std::rc::Rc::new(move || Ok(value.clone())) }
        "constantFrom" => { let values = args; ::std::rc::Rc::new(move || Ok(values[smelt_fc_int(0, values.len() as i64 - 1) as usize].clone())) }
        "oneof" => { let choices = args; ::std::rc::Rc::new(move || smelt_fc_draw(&choices[smelt_fc_int(0, choices.len() as i64 - 1) as usize])) }
        "tuple" => { let items = args; ::std::rc::Rc::new(move || Ok(SmeltUnknown::Array(SmeltArray::new(items.iter().map(smelt_fc_draw).collect::<Result<Vec<_>, _>>()?)))) }
        "array" | "uniqueArray" => {
            let item = options.unwrap_or(SmeltUnknown::Undefined);
            let unique = name == "uniqueArray";
            let settings = args.get(1).cloned();
            ::std::rc::Rc::new(move || {
                let length = smelt_fc_length(settings.as_ref());
                let mut values = Vec::with_capacity(length);
                let mut attempts = 0;
                while values.len() < length && attempts < length * 20 + 20 {
                    attempts += 1;
                    let candidate = smelt_fc_draw(&item)?;
                    if !unique || !values.contains(&candidate) { values.push(candidate); }
                }
                Ok(SmeltUnknown::Array(SmeltArray::new(values)))
            })
        }
        "option" => {
            let item = options.unwrap_or(SmeltUnknown::Undefined);
            let nil = match args.get(1) { Some(SmeltUnknown::Object(settings)) => settings.get("nil").unwrap_or(SmeltUnknown::Null), _ => SmeltUnknown::Null };
            ::std::rc::Rc::new(move || if smelt_fc_int(0, 4) == 0 { Ok(nil.clone()) } else { smelt_fc_draw(&item) })
        }
        "record" => {
            let Some(SmeltUnknown::Object(fields)) = options else { return Err(smelt_throw(SmeltUnknown::String("fast-check: record expects an object of arbitraries".into()))); };
            ::std::rc::Rc::new(move || {
                let object = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
                for (key, arbitrary) in fields.iter() { object.insert(key, smelt_fc_draw(&arbitrary)?); }
                Ok(SmeltUnknown::Object(object))
            })
        }
        "func" => {
            let output = options.unwrap_or(SmeltUnknown::Undefined);
            ::std::rc::Rc::new(move || {
                let output = output.clone();
                let answers = ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections::HashMap::<String, SmeltUnknown>::new()));
                Ok(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
                    let key = format!("{args:?}");
                    if let Some(answer) = answers.borrow().get(&key) { return Ok(answer.clone()); }
                    let answer = smelt_fc_draw(&output)?;
                    answers.borrow_mut().insert(key, answer.clone());
                    Ok(answer)
                })))
            })
        }
        _ => unreachable!("every builder name is listed in smelt_fc_new_namespace"),
    };
    Ok(smelt_fc_arbitrary(generate))
}

/// Run a property: every generated case must neither throw nor answer `false`.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fast_check_run(arbitraries: &SmeltUnknown, predicate: &SmeltUnknown, parameters: Option<&SmeltUnknown>) -> Result<(), Box<dyn std::error::Error>> {
    let predicate = smelt_fc_callable(Some(predicate))?;
    let runs = smelt_fc_option(parameters, "numRuns").unwrap_or(100.0) as usize;
    if let Some(seed) = smelt_fc_option(parameters, "seed") { SMELT_FC_STATE.with(|state| state.set((seed as u64).wrapping_mul(2).wrapping_add(1))); }
    for run in 0..runs {
        let case = match arbitraries {
            SmeltUnknown::Array(items) => items.iter().map(|item| smelt_fc_draw(&item)).collect::<Result<Vec<_>, _>>()?,
            single => vec![smelt_fc_draw(single)?],
        };
        match predicate(case.clone()) {
            Ok(SmeltUnknown::Bool(false)) => return Err(smelt_throw(SmeltUnknown::String(format!("Property failed after {} tests: counterexample {case:?}", run + 1).into()))),
            Ok(_) => {}
            Err(error) => {
                if matches!(smelt_thrown_value(error.as_ref()), SmeltUnknown::Object(object) if object.contains_key("__smelt_fc_precondition")) { continue; }
                return Err(error);
            }
        }
    }
    Ok(())
}

thread_local! { static SMELT_FC_NAMESPACE: SmeltUnknown = smelt_fc_new_namespace(); }

/// The `fc` object.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fast_check_namespace() -> SmeltUnknown { SMELT_FC_NAMESPACE.with(Clone::clone) }

/// Build the `fc` object. A builder it lacks reads as `undefined`, and calling
/// that raises `TypeError` in the test that reached it.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_fc_new_namespace() -> SmeltUnknown {
    let namespace = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
    for name in ["integer", "nat", "float", "double", "boolean", "string", "anything", "constant", "constantFrom", "oneof", "tuple", "array", "uniqueArray", "option", "record", "func"] {
        namespace.insert(name.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| smelt_fc_build(name, args))));
    }
    namespace.insert("property".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(|mut args: Vec<SmeltUnknown>| {
        let predicate = args.pop().unwrap_or(SmeltUnknown::Undefined);
        let property = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
        property.insert("__smelt_fc_arbitraries".to_owned(), SmeltUnknown::Array(SmeltArray::new(args)));
        property.insert("__smelt_fc_predicate".to_owned(), predicate);
        Ok(SmeltUnknown::Object(property))
    })));
    namespace.insert("assert".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(|args: Vec<SmeltUnknown>| {
        let Some(SmeltUnknown::Object(property)) = args.first() else { return Err(smelt_throw(SmeltUnknown::String("fast-check: assert expects a property".into()))); };
        let (Some(arbitraries), Some(predicate)) = (property.get("__smelt_fc_arbitraries"), property.get("__smelt_fc_predicate")) else { return Err(smelt_throw(SmeltUnknown::String("fast-check: assert expects a property".into()))); };
        smelt_fast_check_run(&arbitraries, &predicate, args.get(1))?;
        Ok(SmeltUnknown::Undefined)
    })));
    namespace.insert("pre".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(|args: Vec<SmeltUnknown>| {
        if args.first().is_some_and(smelt_test_truthy) { return Ok(SmeltUnknown::Undefined); }
        let skip = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
        skip.insert("__smelt_fc_precondition".to_owned(), SmeltUnknown::Bool(true));
        Err(smelt_throw(SmeltUnknown::Object(skip)))
    })));
    namespace.insert("sample".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(|args: Vec<SmeltUnknown>| {
        let count = match args.get(1) { Some(SmeltUnknown::Number(count)) => *count as usize, other => smelt_fc_option(other, "numRuns").unwrap_or(10.0) as usize };
        let arbitrary = args.first().cloned().unwrap_or(SmeltUnknown::Undefined);
        Ok(SmeltUnknown::Array(SmeltArray::new((0..count).map(|_| smelt_fc_draw(&arbitrary)).collect::<Result<Vec<_>, _>>()?)))
    })));
    SmeltUnknown::Object(namespace)
}
// @smelt:item-end
