//! Typed absent-value adapters: a `T | null` nested in a container keeps its
//! `null` where the container's own runtime impl would answer `undefined`.
//!
//! A source `T | null` and `T | undefined` both store as a Rust `Option<T>`;
//! the type tells them apart (`Type::Optional(T, Absence)`) and the
//! emitter's own conversions read the tag: a top-level erasure answers `null` (`optional_absent_value`), and so does a
//! list copied element by element, because each element goes through
//! [`FunctionEmitter::erase_rendered`] with its own type. What cannot read the
//! tag is a runtime impl chosen by the RUST type: the live erased view of a
//! list, `Map` or `Set` (`SmeltLiveListWith`, `into_live_smelt_unknown`), the
//! snapshot erasure of a `Map`/`Set`, and every `SmeltInspect` container impl.
//! Through those, `Option<f64>` is one type, and its `None` became `undefined`
//! (`console.log(xs)` of a `(number | null)[]` printed `[ 1, undefined ]`).
//!
//! The adapter is the emitter's typed conversion handed to the runtime as a
//! closure: the runtime's container code takes an element erasure
//! (`into_live_smelt_unknown_with`, `into_smelt_unknown_with`) or an element
//! inspector (`smelt_inspect_list_with` and its siblings), and its plain trait
//! impls are those helpers over the element type's own impl. The emitter passes
//! one only where [`FunctionEmitter::holds_null_absent`] says the element type's
//! rendering differs from its Rust storage type's, so every other program emits
//! exactly what it did.
//!
//! Behavioral requirement: a `null` written into a `T | null` position reads
//! back as `null` through every dynamic boundary and prints as `null`, as in
//! Node (`nullable_containers` parity fixture); `undefined` positions are
//! unchanged. Type-preservation requirement: no value changes its Rust type;
//! the element stays `Option<T>` and the container its typed storage.

use super::*;

impl FunctionEmitter<'_> {
    /// Whether a value of `ty` holds a `T | null` (a `null`-absent optional)
    /// somewhere a Rust-type-chosen runtime impl would convert or print it:
    /// at the top, or inside an optional, a list, a set, a map, a record or a
    /// tuple. A class or a generated union draws and erases its own fields and
    /// arms (their impls are emitted per program and read the tag themselves),
    /// so the walk stops there.
    pub(super) fn holds_null_absent(&self, ty: TypeId) -> bool {
        self.holds_null_absent_in(ty, 0)
    }

    /// [`Self::holds_null_absent`], bounded so a cyclic table terminates.
    fn holds_null_absent_in(&self, ty: TypeId, depth: usize) -> bool {
        if depth > 16 {
            return false;
        }
        if self.mir.types.absent_is_null(ty) {
            return true;
        }
        match self.mir.types.get(ty) {
            Some(Type::Optional(item, _) | Type::List(item) | Type::Set(item)) => {
                self.holds_null_absent_in(*item, depth.saturating_add(1))
            }
            Some(Type::JsMap(key, value) | Type::Dict(key, value)) => {
                self.holds_null_absent_in(*key, depth.saturating_add(1))
                    || self.holds_null_absent_in(*value, depth.saturating_add(1))
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .any(|item| self.holds_null_absent_in(*item, depth.saturating_add(1))),
            Some(
                Type::Bool
                | Type::Int
                | Type::Float
                | Type::FixedInt(_)
                | Type::BigInt
                | Type::PyInt
                | Type::String
                | Type::PyStr
                | Type::Symbol
                | Type::PropKey
                | Type::None
                | Type::Never
                | Type::Unknown
                | Type::IndexedAccess { .. }
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::Union(_)
                | Type::Function(_)
                | Type::Future(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. },
            )
            | None => false,
        }
    }

    /// The erasure of one `ty` element named `param`, as a closure for the
    /// runtime's `_with` container erasures, or `None` when the element type's
    /// own `IntoSmeltUnknown` impl already answers right.
    pub(super) fn element_erasure_closure(
        &self,
        ty: TypeId,
        param: &str,
    ) -> Result<Option<String>, EmitError> {
        if !self.holds_null_absent(ty) {
            return Ok(None);
        }
        let erased = self.erase_rendered(&RustExpr::path(param), ty)?;
        Ok(Some(format!("|{param}| {}", erased.delimited())))
    }

    /// The adapter-taking form of a container erasure `method`
    /// (`.into_live_smelt_unknown()` or `.into_smelt_unknown()`) over
    /// `elements` (a set's member, or a map's key and value): the method
    /// itself when every element erases by its own impl, else its `_with`
    /// twin handed one erasure per element.
    pub(super) fn container_erasure_with(
        &self,
        method: &str,
        elements: &[TypeId],
    ) -> Result<String, EmitError> {
        // A map's elements are its key and value; a set's one element is a value.
        let params = if elements.len() == 2 {
            ["smelt_key", "smelt_value"].as_slice()
        } else {
            ["smelt_value"].as_slice()
        };
        let closures = elements
            .iter()
            .zip(params)
            .map(|(element, param)| self.element_erasure_closure(*element, param))
            .collect::<Result<Vec<_>, _>>()?;
        if closures.iter().all(Option::is_none) {
            return Ok(method.to_owned());
        }
        let live = method == ".into_live_smelt_unknown()";
        let arguments = closures
            .into_iter()
            .map(|closure| {
                let erase =
                    closure.unwrap_or_else(|| "IntoSmeltUnknown::into_smelt_unknown".to_owned());
                // The snapshot form borrows its erasures; the live form keeps them.
                if live { erase } else { format!("&{erase}") }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let base = method.trim_end_matches("()");
        Ok(format!("{base}_with({arguments})"))
    }

    /// How a `ty` value is drawn by `console.log`, as a closure taking the value
    /// by reference, the inspect context and the depth, when its Rust storage
    /// type's `SmeltInspect` impl would draw it wrong; `None` when that impl is
    /// right. A `T | null` draws its absent value as `null`; a container whose
    /// elements need an inspector passes one to its `_with` helper.
    ///
    /// The closure's parameters are typed: it is called directly as well as
    /// handed on as a `SmeltInspector`, and a direct call infers nothing
    /// before its body reads a tuple field.
    pub(super) fn console_inspector(&self, ty: TypeId) -> Result<Option<String>, EmitError> {
        if !self.holds_null_absent(ty) {
            return Ok(None);
        }
        let own = || "SmeltInspect::smelt_inspect".to_owned();
        let body = match self.mir.types.get(ty) {
            Some(&Type::Optional(inner, absence)) => {
                // Each layer draws its own absent value.
                let absent = absence.js_text();
                let present = self
                    .console_inspector(inner)?
                    .map_or_else(own, |inspector| format!("({inspector})"));
                format!(
                    "match smelt_value {{ Some(smelt_value) => {present}(smelt_value, smelt_ctx, smelt_depth), None => {absent:?}.to_owned() }}"
                )
            }
            Some(Type::List(item)) => format!(
                "smelt_inspect_list_with(smelt_value, smelt_ctx, smelt_depth, &{})",
                self.console_inspector(*item)?.unwrap_or_else(own)
            ),
            Some(Type::Set(item)) => {
                let helper = if self.set_container_name(*item) == "SmeltPrimSet" {
                    "smelt_inspect_prim_set_with"
                } else {
                    "smelt_inspect_set_with"
                };
                format!(
                    "{helper}(smelt_value, smelt_ctx, smelt_depth, &{})",
                    self.console_inspector(*item)?.unwrap_or_else(own)
                )
            }
            Some(Type::JsMap(key, value)) => format!(
                "smelt_inspect_map_with(smelt_value, smelt_ctx, smelt_depth, &{}, &{})",
                self.console_inspector(*key)?.unwrap_or_else(own),
                self.console_inspector(*value)?.unwrap_or_else(own)
            ),
            Some(Type::Dict(_, value)) => format!(
                "smelt_inspect_record_with(smelt_value, smelt_ctx, smelt_depth, &{})",
                self.console_inspector(*value)?.unwrap_or_else(own)
            ),
            Some(Type::Tuple(items)) => {
                let entries = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        Ok(match self.console_inspector(*item)? {
                            Some(inspector) => format!(
                                "&SmeltInspectWith {{ value: &smelt_value.{index}, inspect: &{inspector} }} as &dyn SmeltInspect"
                            ),
                            None => format!("&smelt_value.{index} as &dyn SmeltInspect"),
                        })
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                format!("smelt_inspect_array(smelt_ctx, smelt_depth, &[{entries}])")
            }
            Some(
                Type::Bool
                | Type::Int
                | Type::Float
                | Type::FixedInt(_)
                | Type::BigInt
                | Type::PyInt
                | Type::String
                | Type::PyStr
                | Type::Symbol
                | Type::PropKey
                | Type::None
                | Type::Never
                | Type::Unknown
                | Type::IndexedAccess { .. }
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::Union(_)
                | Type::Function(_)
                | Type::Future(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. },
            )
            | None => return Ok(None),
        };
        Ok(Some(format!(
            "|smelt_value: &{}, smelt_ctx: &mut SmeltInspectContext, smelt_depth: usize| -> String {{ {body} }}",
            self.rendered_type(ty)?
        )))
    }

    /// The text a top-level `console.log` of `value` (a reference expression of
    /// type `ty`) prints, for a `ty` that [`Self::holds_null_absent`].
    ///
    /// A top-level optional prints its absent word or its present value as a
    /// top-level argument (a string raw, as `smelt_console_text` prints it);
    /// anything else is drawn by its inspector from a fresh context at depth 0,
    /// as `smelt_console_text` starts one.
    pub(super) fn console_text_of(&self, ty: TypeId, value: &str) -> Result<String, EmitError> {
        // Each optional layer draws its own absent value; a two-layer
        // `T | null | undefined` draws `undefined` outside and `null` inside.
        if let Some(&Type::Optional(inner, absence)) = self.mir.types.get(ty) {
            return Ok(format!(
                "match {value} {{ Some(smelt_value) => {}, None => {:?}.to_owned() }}",
                self.console_text_of(inner, "smelt_value")?,
                absence.js_text()
            ));
        }
        Ok(match self.console_inspector(ty)? {
            Some(inspector) => format!(
                "{{ let mut smelt_ctx = SmeltInspectContext::default(); ({inspector})({value}, &mut smelt_ctx, 0) }}"
            ),
            None => format!("SmeltInspect::smelt_console_text({value})"),
        })
    }
}
