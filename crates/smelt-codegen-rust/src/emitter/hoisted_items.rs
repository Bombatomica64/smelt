//! Rendering the crate-level items that function bodies minted.
//!
//! While bodies render, erasing a named function as a value or recovering a
//! class from an erased record mints a crate-level helper into the context's
//! [`super::render_state::MintedItems`] instead of spelling it inline. Once
//! the last body is written the crate builder asks for all of them at once;
//! which tables exist and how each entry is spelled stays here.

use super::EmitContext;
use crate::runtime_demand::Need;
use crate::{RECORD_ADAPTER_HELPER_PREFIX, RECORD_CONVERSION_HELPER_PREFIX};
use smelt_mir::Mir;

impl EmitContext {
    /// The text of every crate-level item the rendered bodies minted:
    /// function-item identity tokens and value accessors (erased, concrete
    /// erased-function and typed), then the hoisted record conversion and
    /// adapter helpers, each table in its deterministic key order.
    pub(crate) fn hoisted_items_text(&self, mir: &Mir) -> String {
        let mut out = String::new();
        // Flush the per-function-item value accessors collected while erasing
        // function-item-as-value wrappers to `SmeltUnknown`. Each accessor lazily
        // builds and caches ONE erased `SmeltUnknown::Function` so that every
        // reference to the same named function value shares one inner `Rc`, keeping
        // JavaScript reference identity (`===`) stable across references. Sorted by
        // item key via the `BTreeMap` for deterministic golden output.
        // One JavaScript function has several representations: its erased
        // value and a typed `Rc` per callable type it is used at. Each accessor
        // links its allocation to the item's one identity token, so a value
        // erased from ANY of them compares equal (`fs.indexOf(f)` with `fs`
        // holding the erased value and `f` read at its typed accessor answered
        // -1). The token lives in the erased runtime, so it is only minted when
        // that runtime is emitted; without it nothing compares across forms.
        let links_identity = self.demand.has(Need::Unknown);
        let mut identity_keys = std::collections::BTreeSet::new();
        if links_identity {
            identity_keys.extend(self.minted.function_item_accessors.borrow().keys().copied());
            identity_keys.extend(
                self.minted
                    .function_item_typed_accessors
                    .borrow()
                    .keys()
                    .map(|(key, _)| *key),
            );
        }
        // The item's `Function.prototype.name`, carried on its value wrappers.
        let item_names: std::collections::HashMap<usize, &str> = mir
            .closures
            .iter()
            .filter_map(|closure| {
                let name = mir
                    .names
                    .get(closure.name?)
                    .or_else(|| mir.symbols.get(closure.name?))?;
                Some((closure.function_item_key?, name))
            })
            .collect();
        for key in &identity_keys {
            // A `function` declaration is a JavaScript constructor and owns a
            // `prototype`; an arrow or method item does not (see
            // `smelt_hir::Crate::constructible_functions`).
            let fresh = if mir.constructible_function_items.contains(key) {
                "SmeltFunctionIdentity::constructor()"
            } else {
                "SmeltFunctionIdentity::default()"
            };
            let make = match item_names.get(key) {
                Some(name) => format!("{fresh}.named({name:?})"),
                None => fresh.to_owned(),
            };
            out.push_str(&format!(
                "\nfn __smelt_fn_identity_{key}() -> SmeltFunctionIdentity {{\n    thread_local! {{ static SMELT_FN_IDENTITY: SmeltFunctionIdentity = {make}; }}\n    SMELT_FN_IDENTITY.with(Clone::clone)\n}}\n"
            ));
        }
        for (key, body) in self.minted.function_item_accessors.borrow().iter() {
            let linked_body = if links_identity {
                format!(
                    "{{ let smelt_value = {body}; if let SmeltUnknown::Function(smelt_function) = &smelt_value {{ smelt_join_function_identity(smelt_function, &__smelt_fn_identity_{key}()); }} smelt_value }}"
                )
            } else {
                body.clone()
            };
            out.push_str(&format!(
                "\nfn __smelt_fn_value_{key}() -> SmeltUnknown {{\n    thread_local! {{ static SMELT_FN_VALUE: ::std::cell::OnceCell<SmeltUnknown> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_VALUE.with(|smelt_cell| smelt_cell.get_or_init(|| {linked_body}).clone())\n}}\n"
            ));
        }

        // Flush the per-function-item accessors for the CONCRETE `SmeltErasedFunction`
        // type. Each caches ONE `SmeltErasedFunction` so repeated calls of a nullary
        // function-item constant (`doNothing()`/`constant()`) return clones sharing
        // one inner callback `Rc`, keeping JavaScript function-singleton identity.
        // A non-empty collector means a closure was lowered to `SmeltErasedFunction`,
        // so `needs_erased_function` is necessarily true and the struct is emitted.
        for (key, body) in self
            .minted
            .function_item_erased_fn_accessors
            .borrow()
            .iter()
        {
            out.push_str(&format!(
                "\nfn __smelt_fn_erased_{key}() -> SmeltErasedFunction {{\n    thread_local! {{ static SMELT_FN_ERASED: ::std::cell::OnceCell<SmeltErasedFunction> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_ERASED.with(|smelt_cell| smelt_cell.get_or_init(|| {body}).clone())\n}}\n"
            ));
        }

        // Flush the per-function-item accessors for TYPED callable values (see
        // `EmitContext::function_item_typed_accessors`): one cached `Rc` per item
        // and rendered callable type, so every typed reference to a named function
        // shares one identity. The wrapper is bound at its declared type first so
        // the unsizing coercion to `Rc<dyn Fn..>` happens at a `let`; the type is
        // spelled once, as an alias, rather than restated in each of the three
        // positions that name it.
        for ((key, rendered_type), (ordinal, body)) in
            self.minted.function_item_typed_accessors.borrow().iter()
        {
            let link = if links_identity {
                format!(
                    " smelt_join_function_identity(&smelt_function, &__smelt_fn_identity_{key}());"
                )
            } else {
                String::new()
            };
            out.push_str(&format!(
                "\ntype SmeltFnTyped{key}_{ordinal} = {rendered_type};\nfn __smelt_fn_typed_{key}_{ordinal}() -> SmeltFnTyped{key}_{ordinal} {{\n    thread_local! {{ static SMELT_FN_TYPED: ::std::cell::OnceCell<SmeltFnTyped{key}_{ordinal}> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_TYPED.with(|smelt_cell| smelt_cell.get_or_init(|| {{ let smelt_function: SmeltFnTyped{key}_{ordinal} = {body};{link} smelt_function }}).clone())\n}}\n"
            ));
        }

        // Flush the hoisted erased-record-to-class conversions (see
        // `EmitContext::record_conversion_helpers`): each recovered class type's
        // conversion is spelled once here and called from every site.
        for (rendered_type, (ordinal, body)) in
            self.minted.record_conversion_helpers.borrow().iter()
        {
            out.push_str(&format!(
                "\nfn {RECORD_CONVERSION_HELPER_PREFIX}{ordinal}(smelt_value: SmeltUnknown) -> {rendered_type} {{\n    {body}\n}}\n"
            ));
        }
        // The self-reaching record-to-record adapters, likewise spelled once.
        for ((source_text, target_text), (ordinal, body)) in
            self.minted.record_adapter_helpers.borrow().iter()
        {
            out.push_str(&format!(
                "\nfn {RECORD_ADAPTER_HELPER_PREFIX}{ordinal}(smelt_value: {source_text}) -> {target_text} {{\n    {body}\n}}\n"
            ));
        }
        out
    }
}
