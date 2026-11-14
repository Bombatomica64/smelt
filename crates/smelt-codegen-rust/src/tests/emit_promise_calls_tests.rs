//! Generated Rust for promises and erased calls.

use super::*;

/// A `void`-returning callback erased into a callable value returns JavaScript
/// `undefined`, not `null`. Downstream `!== undefined` guards (e.g.
/// cloneDeepWith's customizer wrapper) rely on this to treat the result as "no
/// value" rather than a real `null`.
#[test]
fn emits_void_callback_erasure_as_undefined() {
    let source = source_for(
        r"
type Customizer = (value: unknown, key: unknown) => unknown;
function runWith(fn: Customizer): unknown { return fn(1, 2); }
export function callVoid(): unknown {
  const noop: () => void = () => {};
  return runWith(noop as unknown as Customizer);
}
",
    );

    assert!(
        source.contains(
            "; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Undefined) }"
        ),
        "void callback erasure must yield SmeltUnknown::Undefined, not Null"
    );
}

/// An async return supplies the resolved `T` as the contextual hint for an
/// unparameterized `new Promise`; generated settlement storage must retain it.
#[test]
fn untyped_promise_in_async_return_keeps_concrete_output() {
    let source = source_for(
        r"
async function makeValue(): Promise<number> {
  return new Promise((resolve) => {
    resolve(1);
  });
}
",
    );

    assert!(
        source.contains("Option<Result<f64, Box<dyn std::error::Error>>>"),
        "{source}"
    );
    assert!(
        !source.contains("Option<Result<(), Box<dyn std::error::Error>>>"),
        "{source}"
    );
}

#[test]
fn tuple_asserted_array_literal_renders_as_heterogeneous_tuple() {
    let source = source_for(
        r"
export function pair(promise: Promise<unknown>): [null, Promise<unknown>] {
  return [null, promise] as [null, Promise<unknown>];
}
",
    );
    assert!(source.contains("((), promise)"), "{source}");
    assert!(!source.contains("vec![(), promise]"), "{source}");
}

#[test]
fn named_promise_executor_supplies_resolved_list_type() {
    let source = source_for(
        r"
export function makeQueue(): Promise<number[]> {
  const processor = async (resolve: (value: number[]) => void) => {
    while (true) { return resolve([1, 2]); }
  };
  return new Promise(processor);
}
",
    );
    // The point of this test is that a NAMED executor still gets the resolved list
    // element type (`SmeltList<f64>`) rather than an erased one. That is unchanged;
    // the resolver argument is now passed by shared reference, because `resolve`
    // declares a list parameter and list-typed callback parameters are `&T` (see
    // `callback_param_is_shared_reference`). Both halves of the executor call have to
    // agree on that spelling, which is what radash's `async_` gate exercises at
    // runtime.
    assert!(
        source.contains("Option<Result<SmeltList<f64>, Box<dyn std::error::Error>>>")
            && source.contains("Rc<dyn Fn(&SmeltList<f64>) -> ()>"),
        "{source}"
    );
}

#[test]
fn awaited_conditional_call_uses_asserted_resolved_type_hint() {
    let source = source_for(
        r"
const attempt = <Return>(func: () => Return) => {
  return (): Return extends Promise<any>
    ? Promise<[unknown, unknown]>
    : [unknown, unknown] => undefined as any;
};
export async function run(func: () => Promise<string>): Promise<[unknown, string]> {
  return (await attempt(func)()) as [unknown, string];
}
",
    );
    assert!(
        !source.contains("SmeltUnknown::Array(smelt_tuple_values) = ().clone()"),
        "{source}"
    );
}

/// An async *instance* method is emitted as a synchronous
/// `fn(&self, ..) -> SmeltFuture<T>` whose body already runs inside a moved
/// `async` block, so calling it yields a `SmeltFuture<T>` directly. Unlike a
/// free `async fn` (which yields an `impl Future` needing the stable-ABI
/// wrapper), an instance-method call must NOT be re-wrapped in
/// `SmeltFuture::from_future(Box::pin(..))` — `SmeltFuture<T>` is not a
/// `Future`, so a double wrap fails to compile (E0277).
#[test]
fn async_instance_method_call_is_not_double_wrapped() {
    let source = source_for(
        r"
class Semaphore {
  private available: number = 1;
  async acquire(): Promise<number> {
    return this.available;
  }
}
async function run(): Promise<number> {
  const sema = new Semaphore();
  return await sema.acquire();
}
",
    );

    assert!(
        source.contains("fn acquire(&self) -> SmeltFuture<f64>"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltFuture::from_future(Box::pin(sema.acquire()))"),
        "async instance-method call must not be re-wrapped: {source}"
    );
    assert!(source.contains("sema.acquire()"), "{source}");
}

#[test]
fn promise_bare_delay_settimeout_lowers_to_sleep() {
    // The pure-delay shape `new Promise(resolve => setTimeout(resolve, ms))`
    // resolves `undefined` after the delay and carries no value, so collapsing
    // it to `Sleep` stays correct (and keeps the cheap delay-shim fast path).
    let source = source_for(
        r"
function delay(): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, 5));
}
",
    );

    assert!(source.contains("smelt_sleep_ms"), "{source}");
    assert!(!source.contains("smelt_promise_result"), "{source}");
}

#[test]
fn class_instances_expose_a_per_class_constructor_identity() {
    // The hidden class marker names the class, so an erased instance reads a real
    // `.constructor` and instances of different classes compare unequal.
    //
    // This checks the wiring; `constructor_function_instances_compare_by_class` in
    // `tests/class_identity_runtime.rs` proves the behavior end to end.
    let source = source_for(
        r"
class Alpha { a = 1; }
export function erase(value: Alpha): unknown {
  return value;
}
",
    );

    assert!(
        source.contains("\"__smelt_class\".to_owned(), SmeltUnknown::String(\"Alpha\".into())"),
        "an erased class instance must record its class name in the marker: {source}"
    );
    // The INSTANCE erasure site must name the class. One nameless marker remains
    // by necessity: `smelt_object_from_prototype` stamps a class marker when handed
    // the opaque `"__smelt_proto:class"` sentinel, and that sentinel carries no
    // name, so `Object.create(Object.getPrototypeOf(instance))` cannot recover it.
    // Threading the name through the sentinel is the remaining half of class
    // identity and is tracked separately.
    assert!(
        !source.contains(
            "smelt_object_entries.insert(\"__smelt_class\".to_owned(), SmeltUnknown::Bool(true))"
        ),
        "the erased-instance class marker must not be a nameless boolean: {source}"
    );
    // Interned by name, so two reads for one class are `===` and two classes are not.
    assert!(
        source.contains("fn smelt_class_constructor(class_name: String) -> SmeltUnknown"),
        "the per-class constructor value must be interned by name: {source}"
    );
    assert!(
        source.contains("if field == \"constructor\" && !map.contains_key(\"constructor\")"),
        "`.constructor` must resolve from the class marker without shadowing an own field: {source}"
    );
}

#[test]
fn emits_promise_all_with_tokio_join() {
    let source = source_for(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<[number, number]> {
  return await Promise.all([lift(1), lift(2)]);
}
",
    );

    assert!(source.contains("tokio::join!(biased; _smelt_tmp_0, _smelt_tmp_1)"));
    assert!(source.contains("Ok::<_, Box<dyn std::error::Error>>"));
}

#[test]
fn emits_switch_inside_closure_as_rust_match() {
    let source = source_for(
        "const label = (status: \"pending\" | \"approved\"): string => {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
  }
};
const result = label(\"approved\");
console.log(result);
",
    );

    assert!(
        source.contains("_smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: JsString| {"),
        "{source}"
    );
    assert!(
        source.contains("match closure_arg_0.to_string_lossy().as_str() {"),
        "{source}"
    );
    assert!(source.contains("\"pending\" => {"));
    assert!(source.contains("JsString::from(\"Waiting\")"));
    assert!(source.contains("_ => {"));
}

/// `fetch` emits a future that assembles a whole `Response`.
///
/// Every part comes from the transport: the status, its canonical reason phrase, the
/// response headers, and the body as RAW BYTES — bytes because
/// `SmeltBody::from_text` would stamp an implied `text/plain;charset=UTF-8`,
/// and a fetched response's content type belongs to the server.
///
/// `tests/fetch_response_runtime.rs` proves the round trip against a real
/// socket; this is the cheap half that pins the emitted shape.
#[test]
fn emits_fetch_as_a_response_assembling_future() {
    let source = source_for(
        "async function load(): Promise<Response> {
  return await fetch(\"https://example.com\");
}
",
    );

    assert!(
        source.contains("reqwest::get(JsString::from(\"https://example.com\").to_string()"),
        "{source}"
    );
    assert!(source.contains("SmeltResponse::from_parts"), "{source}");
    assert!(
        source.contains("canonical_reason()"),
        "the reason phrase must come from the response: {source}"
    );
    assert!(
        source.contains("smelt_http.headers().iter()"),
        "the header list must come from the response: {source}"
    );
    assert!(
        source.contains("SmeltBody::from_bytes"),
        "a fetched body must not carry an invented content type: {source}"
    );
}

/// Python's `requests.get(url)` keeps the fused text operation.
///
/// `requests.get(url).text` really is "GET and give me the body", so
/// `HttpGetText` stays for it; only the TypeScript `fetch` spelling moved.
#[test]
fn python_requests_get_keeps_the_fused_text_operation() {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(
        py_frontend::to_hir(
            "import requests\n\ndef load() -> str:\n    return requests.get(\"https://example.com\")\n",
            FileId(0),
            &mut ctx,
        )
        .is_ok(),
        "HIR"
    );
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR");
    smelt_mir::opt::optimize(&mut mir);
    let source = emit_source(&mir).expect("Rust source");
    assert!(source.contains(".text()"), "{source}");
    assert!(
        !source.contains("SmeltResponse"),
        "the Python fused path must not build a Response: {source}"
    );
}

#[test]
fn emits_python_requests_get_as_blocking_reqwest_text() {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(
            py_frontend::to_hir(
                "import requests\n\ndef load() -> str:\n    return requests.get(\"https://example.com\")\n",
                FileId(0),
                &mut ctx,
            )
            .is_ok(),
            "HIR"
        );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    let source = match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    };

    assert!(source.contains(
            "reqwest::blocking::get(\"https://example.com\".to_owned()).expect(\"HTTP GET failed\").text().expect(\"HTTP response body read failed\")"
        ));
}

#[test]
fn injects_reqwest_dependency_for_http_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[
            GeneratedDep::Tokio,
            GeneratedDep::Stdlib(BackendDependency::Reqwest),
        ],
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );

    assert!(manifest.contains("tokio = { version = \"1\""));
    assert!(manifest.contains("reqwest = { version = \"0.12\""));
}

/// A generated crate that emits `genawaiter::rc::Gen` generator bodies must
/// also declare the `genawaiter` dependency: the detection matches the emission
/// condition (`is_generator`), including a generator whose type is erased.
#[test]
fn injects_genawaiter_dependency_for_generator_emission() {
    let mut ctx = HirCtx::new();
    assert!(
        to_hir(
            "function* values(limit: number): Generator<number> {\n\
             \x20 for (let i = 0; i < limit; i += 1) {\n\
             \x20   yield i;\n\
             \x20 }\n\
             }\n\
             function total(): number {\n\
             \x20 let sum = 0;\n\
             \x20 for (const v of values(5)) {\n\
             \x20   sum += v;\n\
             \x20 }\n\
             \x20 return sum;\n\
             }\n",
            FileId(0),
            &mut ctx,
        )
        .is_ok(),
        "HIR"
    );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);

    let source = match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    };
    assert!(
        source.contains("genawaiter::rc::Gen"),
        "generator function should emit a genawaiter state machine: {source}"
    );
    // The runtime prelude that the emitted body references must be defined in
    // the same crate; otherwise the generated code references an undeclared
    // `SmeltGeneratorResult` (the es-toolkit `cargo test --no-run` failure).
    assert!(
        source.contains("pub enum SmeltGeneratorResult"),
        "genawaiter emission must also emit the generator runtime prelude: {source}"
    );

    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &generated_deps(&RuntimeDemand::of(&mir)),
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );
    assert!(
        manifest.contains("genawaiter = \"0.99.1\""),
        "a crate that emits genawaiter bodies must declare the dependency: {manifest}"
    );
}

/// A closure field on a *type-alias* object receiver must stay a closure call
/// even inside callback-migrated bodies: a genuine alias field (remeda's
/// `Funnel.flush`) has no class item for method resolution to find.
#[test]
fn alias_closure_field_call_in_callback_body_stays_closure_call() {
    let source = source_for(
        "type Funnel = {\n\
           readonly call: () => void;\n\
           readonly flush: () => void;\n\
           readonly isIdle: boolean;\n\
         };\n\
         function funnel(cb: () => void): Funnel {\n\
           let idle = true;\n\
           return {\n\
             call: () => { idle = false; cb(); },\n\
             flush: () => { idle = true; cb(); },\n\
             get isIdle() { return idle; },\n\
           };\n\
         }\n\
         export function makeDebouncer(fn: () => void) {\n\
           let cached: number | undefined;\n\
           const debouncingFunnel = funnel(fn);\n\
           return {\n\
             call: () => { debouncingFunnel.call(); return cached; },\n\
             flush: () => { debouncingFunnel.flush(); return cached; },\n\
             get isPending() { return !debouncingFunnel.isIdle; },\n\
           };\n\
         }\n",
    );
    // Reaching here means MIR lowering resolved every call; the alias field
    // dispatches as a stored closure, not a class method item.
    assert_ne!(source, "");
}

/// A `function*` expression stored in an erased slot crosses the dynamic
/// boundary at construction and must erase through the `IntoSmeltUnknown`
/// iterator adapter. The boundary is genuinely dynamic — a live
/// resumable state machine has no concrete/union/generic representation on
/// the erased side, only the iterator protocol the adapter reproduces.
#[test]
fn generator_in_erased_slot_erases_through_into_smelt_unknown() {
    let source = source_for(
        "function length<T>(items: Iterable<T>): number {\n\
           return [...items].length;\n\
         }\n\
         export function countYields(): number {\n\
           return length({\n\
             *[Symbol.iterator]() {\n\
               yield 0;\n\
               yield 1;\n\
               yield 2;\n\
             },\n\
           });\n\
         }\n",
    );
    assert!(
        source.contains("genawaiter::rc::Gen"),
        "generator expression should emit a genawaiter state machine: {source}"
    );
    assert!(
        source.contains(").into_smelt_unknown()"),
        "erased-slot generator must erase through IntoSmeltUnknown: {source}"
    );
    assert!(
        source.contains("impl<Y: IntoSmeltUnknown + 'static, R: IntoSmeltUnknown + Clone + 'static, N: Default + 'static> IntoSmeltUnknown for SmeltGenerator<Y, R, N>"),
        "prelude must define the generator boundary adapter: {source}"
    );
    // Consumers of an erased iterable must drain the iterator protocol the
    // adapter exposes, not just accept a materialized array.
    assert!(
        source.contains("fn smelt_unknown_iterator_items"),
        "list extraction must drain the erased iterator protocol: {source}"
    );
}

#[test]
fn no_profile_sets_panic_abort_while_the_panic_route_exists() {
    // A `throw` from a body that cannot propagate a `Result` travels as a Rust
    // panic and is caught by the enclosing `try`'s `catch_unwind` (see
    // `thrown::emit_panic_route_support`). `panic = "abort"` in the emitted
    // manifest would silently convert every such catchable JavaScript exception
    // into a process abort, so the manifest must never carry it -- in any
    // profile, under any allocator or release-profile combination.
    for allocator in [GeneratedAllocator::System, GeneratedAllocator::Mimalloc] {
        for release_profile in [
            ReleaseProfile::Default,
            ReleaseProfile::Optimized,
            ReleaseProfile::WholeProgram,
        ] {
            let manifest = deps::cargo_toml(
                &EmitOptions::default().crate_name,
                &[GeneratedDep::Tokio, GeneratedDep::Genawaiter],
                allocator,
                &EmitOptions::default().with_release_profile(release_profile),
            );

            assert!(
                !manifest.contains("panic"),
                "emitted manifest must not mention a panic strategy:\n{manifest}"
            );
        }
    }
}

#[test]
fn release_profiles_emit_their_lto_mode() {
    // `optimized` (the default) is thin LTO; `whole-program` is the opt-in fat
    // LTO; `default` leaves Cargo's stock profile alone. Each keeps one codegen
    // unit where it sets a profile at all.
    let manifest = |release_profile| {
        deps::cargo_toml(
            &EmitOptions::default().crate_name,
            &[GeneratedDep::Tokio],
            GeneratedAllocator::System,
            &EmitOptions::default().with_release_profile(release_profile),
        )
    };
    let optimized = manifest(ReleaseProfile::Optimized);
    assert!(
        optimized.contains("[profile.release]\nlto = \"thin\"\ncodegen-units = 1\n"),
        "{optimized}"
    );
    let whole_program = manifest(ReleaseProfile::WholeProgram);
    assert!(
        whole_program.contains("[profile.release]\nlto = \"fat\"\ncodegen-units = 1\n"),
        "{whole_program}"
    );
    let stock = manifest(ReleaseProfile::Default);
    assert!(!stock.contains("[profile.release]"), "{stock}");
}

#[test]
fn injects_serde_json_dependency_for_json_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::SerdeJson)],
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );

    assert!(manifest.contains("serde_json = { version = \"1\", features = [\"preserve_order\"] }"));
}

#[test]
fn injects_rand_dependency_for_random_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Rand)],
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );

    assert!(manifest.contains("rand = \"0.9\""));
}

#[test]
fn injects_regex_dependency_for_regex_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Regex)],
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );

    assert!(manifest.contains("regex = \"1\""));
}

#[test]
fn injects_chrono_dependency_for_date_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Chrono)],
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );

    assert!(manifest.contains("chrono = \"0.4\""));
}

/// A borrowed callback parameter that flows into a callee's owned callback sink
/// (an `Optional<fn(..)>` parameter lowered to `Option<Rc<dyn Fn>>`) must itself
/// be emitted as an owned `Rc<dyn Fn>`, otherwise codegen wraps the non-`'static`
/// borrow in an escaping `Rc::new(..)` adapter that fails borrowck.
#[test]
fn callback_flowing_into_owned_sink_is_owned() {
    let source = source_for(
        r"
function inner(cb?: (x: number) => number): number { return cb ? cb(1) : 0; }
export function outer(cb: (x: number) => number): number { return inner(cb); }
",
    );

    let outer_sig = source
        .lines()
        .find(|line| line.contains("fn outer"))
        .unwrap_or("");
    assert!(
        outer_sig.contains("Rc<dyn Fn"),
        "outer's callback param should be owned: {outer_sig}"
    );
}
