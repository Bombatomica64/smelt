//! The live instance behind an erased view of a reference record.
//!
//! # The defect this closes
//!
//! A reference record (a class or object shape lifted to the
//! `Rc<RefCell<Inner>>` handle representation by `classify::reference_classes`)
//! is ONE JavaScript object: every handle on the cell is the same object. Its
//! erasure to `SmeltUnknown` (`IntoSmeltUnknown`, or the emitter's inline
//! `class_unknown_object_text` adapter) builds an erased object carrying a
//! snapshot of its own-property fields, its prototype members and a stable id
//! derived from the cell address, so reads, method calls and `===` through the
//! erased value already behave. Narrowing that erased value BACK to the record
//! type, however, rebuilt the record from the snapshot
//! (`string_dict_record_adapter_text`): a brand-new cell. Every write the
//! recovered value then received landed on a copy nothing else could see.
//!
//! Hono's `compose` is the shape. Middleware is stored as `Function` in
//! `[[Function, unknown], Params][]`, so `handler(context, next)` erases the
//! `Context` into the call, and the erased adapter of each typed handler
//! `(c: Context, next) => ..` narrows it back to `Context` — a fresh one. The
//! handler's `c.res = ..`, `c.header(..)` and `c.set(..)` were lost to the
//! caller (`compose.test.ts`: `401`, `next() below`, `handler and
//! middlewares`).
//!
//! # The representation
//!
//! What a hand-porting team would write at that seam is a shared reference:
//! the erased value HOLDS the handle, and narrowing downcasts it back. That is
//! exactly this module. The erased object's field store — the `Rc` shared by
//! every handle on that erased object, including the `SmeltRecord` views
//! `smelt_shared_record` hands out — carries an `origin` slot holding a clone
//! of the typed handle as `Box<dyn Any>`:
//!
//! * erasing a reference record stamps its handle into the slot
//!   ([`with_origin_text`]);
//! * narrowing an erased value to a reference record downcasts the slot to the
//!   target handle type first, and only rebuilds from the snapshot when the
//!   value did not come from an erasure of that type ([`restore_or_rebuild_text`]).
//!
//! The slot is strong on purpose. A JavaScript reference keeps its object
//! alive, so an erased view must too: `const x: unknown = new C()` drops the
//! only typed handle at the erasure, and a weak slot (or a registry keyed by
//! id) would then have nothing to hand back. Holding it in the store rather
//! than in a thread-local registry also means the handle lives exactly as long
//! as some erased view of it does, with no table that only grows.
//!
//! # Why not a new `SmeltUnknown` variant
//!
//! The erased object must keep answering every existing object operation —
//! member reads, `for...in`, JSON, structural equality, `instanceof` through
//! `__smelt_class` — and all of those already speak `SmeltUnknown::Object`.
//! A separate variant holding `Rc<dyn Any>` would have to reimplement each of
//! them through reflection. The slot adds only what the snapshot cannot say:
//! WHICH live instance it stands for.
//!
//! # Pay-for-use
//!
//! A program with no reference record never erases one, so the slot and its
//! two accessors are emitted only when `classify::reference_classes` is
//! non-empty; everything else keeps byte-identical output.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the prelude emitter and the emitter shards"
)]

/// The field declaration added to `SmeltFieldStore` when origins are enabled.
pub(crate) const STORE_FIELD_DECL: &str =
    "    /// The live typed value this store is an erased view of, if any (see `smelt_with_origin`).\n    origin: Option<Box<dyn ::std::any::Any>>,";

/// The `SmeltFieldStore` constructor field list suffix for the enabled layout.
///
/// Appended after `index: None` in the store's constructors so a store built
/// by any path — a literal, a JSON parse, a spread copy — starts with no
/// origin: only an erasure of a reference record sets one.
pub(crate) const STORE_FIELD_INIT: &str = ", origin: None";

/// The `SmeltObject` methods that stamp and read the origin slot.
pub(crate) const OBJECT_METHODS: &str = "    /// Record the live typed value this erased object is a view of.\n    ///\n    /// Called by a reference record's erasure with a clone of its handle, so\n    /// narrowing the erased object back hands out the SAME instance rather than\n    /// a copy rebuilt from the field snapshot.\n    #[allow(dead_code)]\n    fn smelt_with_origin<T: 'static>(self, origin: T) -> Self { self.store.borrow_mut().origin = Some(Box::new(origin)); self }\n    /// The live typed value this erased object is a view of, when it is a `T`.\n    #[allow(dead_code)]\n    fn smelt_origin<T: Clone + 'static>(&self) -> Option<T> { self.store.borrow().origin.as_ref().and_then(|origin| origin.downcast_ref::<T>()).cloned() }";

/// The `SmeltRecord` accessor reading the same slot through a record handle.
///
/// A record recovered at `Record<string, unknown>` shares the erased object's
/// store (`smelt_shared_record`), so the origin survives that view too.
pub(crate) const RECORD_IMPL: &str = "impl<K, V> SmeltRecord<K, V> {\n    /// The live typed value this record's store is an erased view of, when it is a `T`.\n    #[allow(dead_code)]\n    fn smelt_origin<T: Clone + 'static>(&self) -> Option<T> { self.store.borrow().origin.as_ref().and_then(|origin| origin.downcast_ref::<T>()).cloned() }\n}";

/// Stamp `handle` (an expression yielding the typed reference handle) as the
/// origin of the erased object built by `object_ctor`.
pub(crate) fn with_origin_text(object_ctor: &str, handle: &str) -> String {
    format!("{object_ctor}.smelt_with_origin({handle}.clone())")
}

/// Recover a reference record from the record view `record`: the live handle
/// when the view came from an erasure of that record type, else `rebuild`.
///
/// The handle type is inferred from `rebuild`'s type — both arms of the match
/// must agree — so a generic record needs no spelled type arguments here.
pub(crate) fn restore_or_rebuild_text(record: &str, rebuild: &str) -> String {
    format!(
        "match {record}.smelt_origin() {{ Some(smelt_origin) => smelt_origin, None => {rebuild} }}"
    )
}

/// Emit `SmeltFromUnknown` for a reference record's handle type.
///
/// The inbound mirror of the reference erasure, and the reason it can exist at
/// all: recovering a handle from a field snapshot would mint a NEW cell, which
/// is not narrowing but copying, so reference records used to have no inbound
/// impl. With the origin slot the recovery hands back the live handle an
/// erasure stamped, and only a value that never was one of these instances (a
/// literal, a JSON value, a spread copy) is rebuilt — into a fresh cell, which
/// is exactly what that value is: a different object.
///
/// Every lifted type parameter is bounded by `SmeltFromUnknown`, so a record
/// without this impl cannot instantiate a generic at all; a class lifted to a
/// handle because it is compared by identity (`classify`) would otherwise stop
/// compiling in every generic helper it was passed to (es-toolkit's `meanBy`
/// spec over `Person[]`).
///
/// Fields are rebuilt under the same whitelist as the value-record recovery
/// (`type_supports_from_unknown`); anything else keeps its `Default`.
pub(crate) fn emit_reference_from_smelt_unknown_impl(
    writer: &mut crate::rust::CodeWriter,
    mir: &smelt_mir::Mir,
    context: &crate::emitter::EmitContext,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
) {
    let mut assigns = Vec::new();
    for field in fields {
        if !field.visibility.is_own_property()
            || !crate::type_supports_from_unknown(mir, context, field.ty)
        {
            continue;
        }
        // Read the SOURCE spelling out of the erased object; write the
        // sanitized Rust field, as the value-record recovery does.
        let field_name = crate::rust::RustIdent::new(mir.symbols.get(field.name).unwrap_or("field"))
            .into_string();
        let key = mir
            .names
            .get(field.name)
            .or_else(|| mir.symbols.get(field.name))
            .unwrap_or("field");
        assigns.push(format!(
            "if let Some(field) = object.get({key:?}) {{ result.0.borrow_mut().{field_name} = SmeltFromUnknown::smelt_from_unknown(field); }}"
        ));
    }
    writer.line("/// Recover this reference record from an erased value: the live instance");
    writer.line("/// when the value is an erasure of one, else a fresh object rebuilt from it.");
    writer.block(
        format!("impl{impl_generics} SmeltFromUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn smelt_from_unknown(value: SmeltUnknown) -> Self", |fn_writer| {
                fn_writer.line("let SmeltUnknown::Object(object) = value else { return Self::default() };");
                fn_writer.line("if let Some(origin) = object.smelt_origin::<Self>() { return origin; }");
                fn_writer.line("let result = Self::default();");
                for assign in &assigns {
                    fn_writer.line(assign);
                }
                fn_writer.line("result");
            });
        },
    );
}
