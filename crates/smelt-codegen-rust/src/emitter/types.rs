//! Rendering a MIR type as Rust: `rust_type`, the scoped and lexical forms of it,
//! and the small type lookups rendering needs.

use super::*;
use crate::runtime_demand::Need;
use crate::rust::RustExpr;
use crate::rust::RustShape;
use crate::rust::RustType;
use smelt_hir::{ErasureReason, FunctionType};

/// Whether a callback parameter declared mutable gets a `&mut ` prefix.
///
/// The callback-argument renderers differ here, and the difference is
/// emitted bytes: `param_type`, the default-callback renderer, the
/// virtual-method storage field and the rendered shape adapter apply it; the
/// three caller-scope adapters bypass `function_type_param` and do not. It is
/// an explicit axis because normalizing it either way changes generated output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MutablePrefix {
    /// Prefix `&mut ` for indices in `FunctionType::mutable_params`.
    Apply,
    /// Never prefix, whatever `mutable_params` says.
    Ignore,
}

impl FunctionEmitter<'_> {
    /// Render one parameter type from a function-typed value.
    ///
    /// `FunctionType::mutable_params` is a Rust ABI marker used for structural
    /// method fields that must preserve JavaScript object mutation through a
    /// callback boundary.
    pub(super) fn function_type_param(
        &self,
        function: &FunctionType,
        index: usize,
        param: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustType, EmitError> {
        let param_text = self.rust_type(param, false, substitution)?;
        if self.param_passed_by_mut_ref(function, index) {
            Ok(RustType::raw(format!("&mut {param_text}")))
        } else if self.callback_param_is_shared_reference(function, index, param) {
            Ok(RustType::raw(format!("&{param_text}")))
        } else {
            Ok(param_text)
        }
    }

    /// Converts a Python-style element index into a Rust `usize` expression.
    ///
    /// Negative indexes are offset from the collection length. Bounds are not
    /// clamped because Python element indexing raises when the normalized index
    /// is still outside the collection; the generated Rust keeps that behavior
    /// with `expect` on negative conversion and the eventual indexed lookup.
    /// The id of `needle`, whether or not the program spells it.
    ///
    /// Infallible in practice; the `Result` keeps the signature every caller
    /// composes with `?`. To ask whether the program spells a type, use
    /// [`Self::find_type_id`].
    pub(super) fn type_id(&self, needle: Type) -> Result<TypeId, EmitError> {
        Ok(self.mir.types.intern_late(needle))
    }

    /// The erased type a `place_ty` fallback degrades to, named at the site.
    ///
    /// The `_ =>` arms of [`Self::place_ty`] answer "this read has no static
    /// type", which the emitter spells as the erased carrier. Asking for it
    /// through [`Self::type_id`] made that answer depend on something unrelated
    /// to the read: a crate with no erased value anywhere has no `Type::Unknown`
    /// entry at all, so the lookup failed with `type table does not contain
    /// literal operand type Unknown at emitter/types.rs:<line>` — a blocker
    /// naming a line of the COMPILER rather than the read in the user's program
    /// (H14).
    ///
    /// The obvious repair is rejected on purpose: interning `Unknown` here would
    /// flip `Need::Unknown` for the whole crate and emit the entire
    /// erased prelude for a program that has no erased value, so a typing
    /// fallback would change what every other decision in the crate answers.
    ///
    /// So the fallback DEGRADES AT THE SITE instead. When the crate already
    /// interns the erased carrier the answer is exactly what it was, so every
    /// existing emission is byte-identical; when it does not, the blocker names
    /// the read — the receiver's rendered type and the member being read — and
    /// `describe` is evaluated only on that failing path. The enclosing function
    /// is attached by the emit driver's `with_site`.
    pub(super) fn erased_fallback_ty(
        &self,
        describe: impl FnOnce() -> String,
    ) -> Result<TypeId, EmitError> {
        self.find_type_id(&Type::Unknown).ok_or_else(|| {
            EmitError::new(format!(
                "cannot type {}: the read has no statically known type, and this crate interns no `unknown`, so there is no erased carrier to answer with",
                describe()
            ))
        })
    }

    /// Renders a receiver's type for a blocker message, never failing itself.
    ///
    /// Used only on the error path of [`Self::erased_fallback_ty`], so a type
    /// that cannot be rendered falls back to its debug shape rather than
    /// replacing the blocker being reported with a second one.
    pub(super) fn blocker_type_text(&self, ty: TypeId) -> String {
        self.rendered_type(ty)
            .unwrap_or_else(|_err| RustType::from(format!("{:?}", self.mir.types.get(ty))))
            .into_string()
    }

    /// Finds the type ID for a type the program has: one in the MIR table, or
    /// the erased carrier when an operation answers through it.
    pub(super) fn find_type_id(&self, needle: &Type) -> Option<TypeId> {
        if *needle == Type::Unknown && self.context.demand().has(Need::ErasedAnswer) {
            return Some(self.mir.types.intern_late(Type::Unknown));
        }
        let type_index = self.mir.types.all().iter().position(|ty| ty == needle)?;
        let compact = compact_index(type_index, "type index does not fit u32").ok()?;
        Some(TypeId(compact))
    }

    /// Return the innermost non-optional type for defensive Rust `Option<T>` emission.
    pub(super) fn flatten_optional_inner(&self, mut ty: TypeId) -> TypeId {
        while let Some(Type::Optional(inner)) = self.mir.types.get(ty) {
            ty = *inner;
        }
        ty
    }

    /// Renders a type in the scope of the current function.
    pub(super) fn lexical_rust_type(
        &self,
        ty: TypeId,
        allow_impl_trait: bool,
    ) -> Result<RustType, EmitError> {
        let scope = self.current_function_type_params();
        self.rust_type(ty, allow_impl_trait, &TypeSubstitution::lexical(&scope))
    }

    /// Converts a type ID to its Rust text in a nested or value position.
    pub(super) fn rendered_type(&self, ty: TypeId) -> Result<RustType, EmitError> {
        let rendered = self.lexical_rust_type(ty, false)?;
        if rendered.holds_carrier() {
            self.context.note_carrier_use();
        }
        Ok(rendered)
    }

    /// Converts a type ID to its Rust text at a signature root, where a callable
    /// parameter can be spelled `impl Fn(..)`.
    pub(super) fn signature_type(&self, ty: TypeId) -> Result<RustType, EmitError> {
        self.lexical_rust_type(ty, true)
    }

    /// Renders a template that spells the erased carrier, recording that it did.
    ///
    /// Every such template goes through here, so the generic trial learns what a
    /// body touches at the point the text is made. It takes the
    /// [`CarrierUse`] proof ([`Self::carrier`]): a carrier template cannot be
    /// written without asking the runtime demand, and asking fails loudly at
    /// the site when the demand scan did not predict the carrier.
    ///
    /// [`CarrierUse`]: crate::runtime_demand::CarrierUse
    pub(super) fn carrier_format(
        &self,
        _carrier: crate::runtime_demand::CarrierUse,
        template: std::fmt::Arguments<'_>,
    ) -> String {
        self.context.note_carrier_use();
        template.to_string()
    }

    /// [`Self::carrier_format`] for a template with no parameters.
    pub(super) fn carrier_expr(
        &self,
        _carrier: crate::runtime_demand::CarrierUse,
        text: &RustExpr,
    ) -> RustExpr {
        self.context.note_carrier_use();
        text.clone()
    }

    /// The proof that this program carries the erased runtime, for a carrier
    /// template. See [`crate::runtime_demand::RuntimeDemand::carrier`].
    pub(super) fn carrier(&self) -> Result<crate::runtime_demand::CarrierUse, EmitError> {
        self.context.demand().carrier()
    }

    /// What `ty` renders to in the scope of the current function.
    pub(super) fn rust_shape(&self, ty: TypeId) -> Result<RustShape, EmitError> {
        Ok(self.lexical_rust_type(ty, false)?.shape())
    }

    /// Whether `ty` renders to the runtime carrier in the current scope.
    pub(super) fn renders_erased(&self, ty: TypeId) -> Result<bool, EmitError> {
        Ok(self.rust_shape(ty)? == RustShape::Erased)
    }

    /// Return whether `param` is a type parameter in scope for the current
    /// emitted function.
    ///
    /// Class constructors and methods are emitted inside `impl<T> Class<T>`
    /// blocks, so class type parameters are in scope for them. A generic free
    /// function (`fn identity<T>(x: T) -> T`) declares its own type parameters;
    /// those are in scope only within that function. Either way, an in-scope
    /// parameter keeps its generic shape instead of erasing to `SmeltUnknown`.
    pub(super) fn current_function_has_type_param(&self, param: Symbol) -> bool {
        self.current_function_type_params().contains(&param)
    }

    /// Return the type parameters in scope for the current emitted function.
    ///
    /// For a class member this is the owning class's generic parameters (the
    /// member is emitted inside the class `impl<T>` block). For a module-level
    /// free function it is the function's own declared type parameters, which
    /// lets a generic free function emit real Rust generics rather than routing
    /// its `T`-typed parameters and return through the runtime unknown carrier.
    pub(super) fn current_function_type_params(&self) -> HashSet<Symbol> {
        // Text destined for a hoisted module-level item may not spell any type
        // parameter: the item declares none. See
        // `FunctionEmitter::hoisted_module_item`.
        if self.hoisted_module_item.get() {
            return HashSet::new();
        }
        // A closure (or other nested) sub-emitter inherits the type parameters
        // that are in scope in the enclosing Rust output. That set was captured
        // from the enclosing emitter at construction time, so it is already
        // gated by the enclosing function's suppress/erasure decision: it is
        // empty when the enclosing signature erased its generics, and holds the
        // in-scope `T`s otherwise. Honoring it keeps closure bodies rendering
        // `T` exactly when the enclosing signature declares it.
        //
        // An EMPTY enclosing set must not fall through to the origin rule below,
        // which for a closure's synthetic `HirOrigin::Body` function (declaring
        // no type parameters) answers the same empty set.
        //
        // A closure of an inherited copy is still spelled in the declaring
        // base's scope, so that base's parameters are spellable in it too:
        // `rust_type` renders each one as its argument in the receiving class.
        if let emit_role::EmitRole::Closure(scope) = &self.role {
            let mut params = scope.enclosing_type_params.clone();
            params.extend(scope.inherited_type_args.keys().copied());
            return params;
        }
        let class_name = match self.function.origin {
            // A base initializer copy renders in the impl it is emitted into,
            // whose flattened layout is what its field writes must agree with.
            HirOrigin::ClassConstructor { .. }
                if let Some(receiver) = self.role.base_initializer() =>
            {
                receiver.owner
            }
            // An inherited copy renders in the receiving class's `impl`; the
            // declaring base's parameters are mapped by the role's substitution.
            HirOrigin::ClassMethod { .. } if let Some(owner) = self.role.inherited_impl_owner() => {
                owner
            }
            HirOrigin::ClassConstructor { class, .. }
            | HirOrigin::ClassMethod { class, .. }
            | HirOrigin::ClassStaticMethod { class, .. } => class,
            HirOrigin::Body(_) => {
                // A free function emits real Rust generics only when its
                // signature is generic-safe (see
                // `classes::function_emits_rust_generics`) AND the body-cleanliness
                // trial has not forced a fall back to erasure via
                // `OwnGenerics::Erased`.
                if self.role.own_generics_erased() {
                    return HashSet::new();
                }
                // This is the *subset* that lifts, not all-or-nothing. A parameter absent
                // from it lowers through the same erasure path a fully-erased function uses.
                return crate::generic_lifting::liftable_type_params(
                    self.mir,
                    self.function,
                    &self.context.owned_callback_params,
                    &self.context.bound_traits,
                    &self.context.erased_arguments,
                );
            }
        };
        crate::classes::find_class(self.mir, class_name)
            .map(|class| class.type_params.iter().map(|param| param.name).collect())
            .unwrap_or_default()
    }

    /// The one canonical, substitution-aware Rust type lowering entry point.
    ///
    /// Every structural `Type` -> Rust-type-text decision in this crate lives in
    /// this match; every other renderer is either a wrapper around it or a
    /// function-signature assembler that calls back into it. Whether a
    /// `Type::TypeParam` keeps its generic shape or erases to the runtime unknown
    /// carrier is decided in exactly one place — `substitution.resolve` — so a
    /// render position changes what it erases by changing the environment it
    /// passes in, never by rendering types itself.
    ///
    /// `allow_impl_trait` stays a separate argument rather than folding into the
    /// substitution: it is a property of the render *position* (root versus
    /// nested), not of the type-parameter environment.
    pub(super) fn rust_type(
        &self,
        ty: TypeId,
        allow_impl_trait: bool,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustType, EmitError> {
        let resolved_ty = self
            .mir
            .types
            .get(ty)
            .ok_or_else(|| EmitError::new("MIR references an unknown type"))?;
        match resolved_ty {
            Type::Bool => Ok(RustType::raw("bool")),
            Type::Int => Ok(RustType::raw("i64")),
            Type::FixedInt(width) => Ok(RustType::raw(width.rust_name())),
            Type::BigInt => Ok(RustType::raw(crate::bigint_prelude::BIGINT_TYPE)),
            Type::Float => Ok(RustType::raw("f64")),
            Type::String => Ok(RustType::raw("JsString")),
            Type::PyStr => Ok(RustType::raw("String")),
            Type::Symbol => Ok(RustType::raw("SmeltSymbol")),
            Type::PropKey => Ok(RustType::raw("SmeltPropKey")),
            Type::Unknown => Ok(RustType::erased()),
            Type::Never => {
                self.note_backend_erasure(ErasureReason::NeverValue, String::new());
                Ok(RustType::erased())
            }
            // A declaring base's parameter inside an inherited copy renders as
            // its argument in the receiving class's scope. Call-site bindings
            // (a callee's own parameters) never consult the map.
            Type::TypeParam { name }
                if !substitution.has_bindings()
                    && let Some(bound) = self.inherited_bound(*name) =>
            {
                self.with_inherited_bound(|| self.rust_type(bound, allow_impl_trait, substitution))
            }
            Type::TypeParam { name } => {
                self.type_param_rust_type(*name, substitution, allow_impl_trait)
            }
            Type::Class { name, args } => self.class_rust_type(*name, args, substitution),
            Type::None => Ok(RustType::raw("()")),
            Type::List(item) => {
                let item = self.rust_type(*item, false, substitution)?;
                Ok(RustType::composed(format!("SmeltList<{item}>"), [&item]))
            }
            // An insertion-ORDERED set, because JavaScript specifies `Set`
            // iteration as insertion order and a Rust `HashSet` has none (H52).
            // `SmeltPrimSet` keeps a `Vec` of entries beside a hash index, so
            // membership stays hashed; the sibling `SmeltJsSet` is ordered too
            // but erases each element for SameValueZero membership, which would
            // drag the whole `SmeltUnknown` carrier into any program holding a
            // `Set<string>`. `set_container_name` picks between the two.
            Type::Set(item) => {
                let container = self.set_container_name(*item);
                let item = self.rust_type(*item, false, substitution)?;
                Ok(RustType::composed(format!("{container}<{item}>"), [&item]))
            }
            Type::Dict(key_ty, value_ty) if self.dict_uses_smelt_record(*key_ty) => {
                let key = self.rust_type(*key_ty, false, substitution)?;
                let value = self.rust_type(*value_ty, false, substitution)?;
                Ok(RustType::composed(
                    format!("SmeltRecord<{key}, {value}>"),
                    [&key, &value],
                ))
            }
            // A property-key record is an object keyed by `ToPropertyKey`
            // results; its runtime type seals the key (see `prop_key.rs`).
            Type::Dict(key, value) if self.is_prop_key_record_key(*key) => {
                let value_ty = self.rust_type(*value, false, substitution)?;
                Ok(RustType::composed(
                    self.carrier_format(
                        self.carrier()?,
                        format_args!("SmeltPropKeyRecord<{value_ty}>"),
                    ),
                    [&value_ty],
                )
                .on_carrier())
            }
            Type::Dict(key, value) if self.dict_uses_js_key_map(*key) => {
                let key = self.rust_type(*key, false, substitution)?;
                let value = self.rust_type(*value, false, substitution)?;
                Ok(RustType::composed(
                    self.carrier_format(
                        self.carrier()?,
                        format_args!("SmeltJsMap<{key}, {value}>"),
                    ),
                    [&key, &value],
                )
                .on_carrier())
            }
            // Insertion-ordered, as a Python dict and a JavaScript object are;
            // see `dict_renders_as_index_map`.
            Type::Dict(key, value) => {
                let key = self.rust_type(*key, false, substitution)?;
                let value = self.rust_type(*value, false, substitution)?;
                Ok(RustType::composed(
                    format!("::indexmap::IndexMap<{key}, {value}>"),
                    [&key, &value],
                ))
            }
            // A source-spelled `Map` always uses the `SmeltJsMap` container, even
            // when string-keyed. Unlike a string-keyed `Record` (which uses
            // `SmeltRecord`), `SmeltJsMap`'s `IntoSmeltUnknown` stamps the
            // `__smelt_map` marker so erased `Map`s remain observable as Maps
            // (`isMap`, `[object Map]`, structural `isEqual`).
            Type::JsMap(key, value) => {
                let key = self.rust_type(*key, false, substitution)?;
                let value = self.rust_type(*value, false, substitution)?;
                Ok(RustType::composed(
                    self.carrier_format(
                        self.carrier()?,
                        format_args!("SmeltJsMap<{key}, {value}>"),
                    ),
                    [&key, &value],
                )
                .on_carrier())
            }
            Type::Tuple(items) => {
                let items = items
                    .iter()
                    .map(|item| self.rust_type(*item, false, substitution))
                    .collect::<Result<Vec<_>, _>>()?;
                let items_text = items
                    .iter()
                    .map(RustType::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                let text = if items.len() == 1 {
                    format!("({items_text},)")
                } else {
                    format!("({items_text})")
                };
                Ok(RustType::composed(text, &items))
            }
            Type::Optional(item) => {
                let item =
                    self.rust_type(self.flatten_optional_inner(*item), false, substitution)?;
                Ok(RustType::composed(format!("Option<{item}>"), [&item]))
            }
            // The generated enum's arguments resolve through the same
            // substitution as every other type parameter at this position, so
            // a parameter free here (a callable type's own `<T>`, an erased
            // item's generics) erases instead of naming an undeclared type.
            Type::Union(_) if self.concrete_union_members(ty).is_some() => {
                self.union_rust_type_in(ty, substitution)
            }
            Type::Union(arms) => {
                self.note_backend_erasure(
                    ErasureReason::ErasedUnion,
                    format!("union of {} arms", arms.len()),
                );
                Ok(RustType::erased())
            }
            Type::Function(function) => {
                self.function_rust_type(function, allow_impl_trait, substitution)
            }
            // A source `Promise<T>` / `Type::Future(T)` lowers to the generic
            // promise-value ABI `SmeltFuture<T>` in every position, so the same
            // MIR future type renders one Rust type everywhere.
            Type::Future(item) => {
                let item = self.rust_type(*item, false, substitution)?;
                Ok(RustType::composed(format!("SmeltFuture<{item}>"), [&item]))
            }
            Type::Generator {
                is_async,
                yield_ty,
                return_ty,
                next_ty,
            } => {
                let yielded = self.rust_type(*yield_ty, false, substitution)?;
                let returned = self.rust_type(*return_ty, false, substitution)?;
                let next = self.rust_type(*next_ty, false, substitution)?;
                let flavour = if *is_async { "Async" } else { "" };
                Ok(RustType::composed(
                    format!("Smelt{flavour}Generator<{yielded}, {returned}, {next}>"),
                    [&yielded, &returned, &next],
                ))
            }
            Type::GeneratorResult {
                yield_ty,
                return_ty,
            } => {
                let yielded = self.rust_type(*yield_ty, false, substitution)?;
                let returned = self.rust_type(*return_ty, false, substitution)?;
                Ok(RustType::composed(
                    format!("SmeltGeneratorResult<{yielded}, {returned}>"),
                    [&yielded, &returned],
                ))
            }
        }
    }

    /// Converts a function return type to Rust, including uncaught exception wrapping.
    pub(super) fn return_type(&self, ty: TypeId) -> Result<RustType, EmitError> {
        let inner = self.rendered_type(ty)?;
        if (self.function.is_async || self.function.can_throw) && !self.function.is_generator {
            Ok(RustType::raw(format!("Result<{inner}, {ERROR_BOX}>")))
        } else {
            Ok(inner)
        }
    }

    /// Return the value type produced by `return` inside the current body.
    pub(super) fn body_return_ty(&self) -> TypeId {
        match (
            self.function.is_generator,
            self.mir.types.get(self.function.return_ty),
        ) {
            (true, Some(Type::Generator { return_ty, .. })) => *return_ty,
            _ => self.function.return_ty,
        }
    }

    // Gets the entry block of the function.

    /// Find the ID of an already interned type.
    pub(super) fn existing_type_id(&self, ty: Type) -> Option<TypeId> {
        self.mir
            .types
            .all()
            .iter()
            .position(|candidate| *candidate == ty)
            .and_then(|index| u32::try_from(index).ok())
            .map(TypeId)
    }

    /// A type parameter: its binding, its generic name or the erased carrier.
    fn type_param_rust_type(
        &self,
        name: Symbol,
        substitution: &TypeSubstitution<'_>,
        allow_impl_trait: bool,
    ) -> Result<RustType, EmitError> {
        match substitution.resolve(name)? {
            Resolved::Spelled(param) => Ok(RustType::with_shape(
                crate::names::type_param(self.symbol_name(param)?).into_string(),
                RustShape::Generic,
            )),
            // The substituted type is a *caller-side* type: its own type
            // parameters live in the caller's world, not the callee's, so
            // the callee bindings must not keep applying inside it.
            Resolved::Substituted(bound) => {
                // A bound that is itself one of the CALLER's real generics
                // (`inner(xs)` inside `outer<T>` binds `inner`'s parameter
                // to `outer`'s `T`) is spelled in the caller's own lexical
                // scope, which is where the call site is rendered.
                let caller_scope = self.current_function_type_params();
                let rendered = if matches!(
                    self.mir.types.get(bound),
                    Some(Type::TypeParam { name: bound_name }) if caller_scope.contains(bound_name)
                ) {
                    self.rust_type(
                        bound,
                        allow_impl_trait,
                        &TypeSubstitution::lexical(&caller_scope),
                    )?
                } else {
                    self.rust_type(bound, allow_impl_trait, &substitution.without_bindings())?
                };
                // Seam 5 (`emitter::seam_assertions`): the binding said
                // Concrete, so the rendering must spell that type. The text
                // is already built here, so the check renders nothing twice.
                #[cfg(debug_assertions)]
                self.debug_assert_substituted_binding_is_not_erased(name, bound, &rendered);
                Ok(rendered)
            }
            Resolved::Erased => {
                let param = self.symbol_name(name).unwrap_or("?").to_owned();
                self.note_backend_erasure(ErasureReason::ErasedGeneric, param);
                Ok(RustType::erased())
            }
        }
    }

    /// A class or interface, including the host classes spelled by their runtime type.
    fn class_rust_type(
        &self,
        name: Symbol,
        args: &[TypeId],
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustType, EmitError> {
        if self.is_regexp_class_symbol(name)? {
            return Ok(RustType::raw("SmeltRegExp"));
        }
        // Both synthetic match-result classes are backed by the same
        // concrete `SmeltMatch` Rust type.
        if self.is_match_class_symbol(name)? {
            return Ok(RustType::raw("SmeltMatch"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::Headers) {
            return Ok(RustType::raw("SmeltHeaders"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::PyException) {
            return Ok(RustType::raw("SmeltPyException"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::PyExceptionArgs) {
            return Ok(RustType::raw("SmeltPyExceptionArgs"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::UrlSearchParams) {
            return Ok(RustType::raw("SmeltUrlSearchParams"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::Url) {
            return Ok(RustType::raw("SmeltUrl"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::FormData) {
            return Ok(RustType::raw("SmeltFormData"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::Response) {
            return Ok(RustType::raw("SmeltResponse"));
        }
        // A body HANDLE is the generated body type itself: the
        // modeled `ReadableStream` is that handle and nothing else.
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::ReadableStream) {
            return Ok(RustType::raw("SmeltBody"));
        }
        if let Some(codec_type) = match self.stdlib_class_of_symbol(name)? {
            Some(smelt_stdlib::StdlibClass::TextEncoder) => Some("SmeltTextEncoder"),
            Some(smelt_stdlib::StdlibClass::TextDecoder) => Some("SmeltTextDecoder"),
            Some(smelt_stdlib::StdlibClass::NodeHash) => Some("SmeltHash"),
            // The whole typed-array family is ONE Rust type: the
            // element kind is a runtime field of the value, not part of
            // its static identity, so all eleven source spellings and
            // the synthetic codec name render the same type.
            Some(smelt_stdlib::StdlibClass::TypedArray) => Some("SmeltTypedArray"),
            Some(smelt_stdlib::StdlibClass::ArrayBuffer) => Some("SmeltArrayBuffer"),
            // `DataView` is its own type, not a twelfth kind: its
            // element width is an argument of each accessor rather
            // than a field of the value.
            Some(smelt_stdlib::StdlibClass::DataView) => Some("SmeltDataView"),
            Some(smelt_stdlib::StdlibClass::CryptoKey) => Some("SmeltCryptoKey"),
            // A `Date` is a shared, mutable time value (`smelt-runtime`'s `date.rs`).
            Some(smelt_stdlib::StdlibClass::Date) => Some("SmeltDate"),
            _ => None,
        } {
            return Ok(RustType::raw(codec_type));
        }
        // `Blob` and `File` are one Rust type: a file is a blob whose
        // name is present.
        if self
            .stdlib_class_of_symbol(name)?
            .is_some_and(smelt_stdlib::StdlibClass::is_blob_runtime_type)
        {
            return Ok(RustType::raw("SmeltBlob"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::Request) {
            return Ok(RustType::raw("SmeltRequest"));
        }
        if self.stdlib_class_of_symbol(name)? == Some(smelt_stdlib::StdlibClass::EventEmitter) {
            return Ok(RustType::raw("SmeltEventEmitter"));
        }
        if let Some(http_type) = match self.stdlib_class_of_symbol(name)? {
            Some(smelt_stdlib::StdlibClass::HttpServer) => Some("SmeltHttpServer"),
            Some(smelt_stdlib::StdlibClass::IncomingMessage) => Some("SmeltIncomingMessage"),
            Some(smelt_stdlib::StdlibClass::ServerResponse) => Some("SmeltServerResponse"),
            Some(smelt_stdlib::StdlibClass::SqliteDatabase) => Some("SmeltSqliteDatabase"),
            Some(smelt_stdlib::StdlibClass::SqliteStatement) => Some("SmeltSqliteStatement"),
            _ => None,
        } {
            return Ok(RustType::raw(http_type));
        }
        if !self.mir.has_class(name)
            && !self
                .mir
                .interfaces
                .iter()
                .any(|interface| interface.name == name)
        {
            let class = self.symbol_name(name).unwrap_or("?").to_owned();
            self.note_backend_erasure(ErasureReason::UnresolvedClass, class);
            return Ok(RustType::erased());
        }
        let type_name = self.type_rust_name(name)?;
        // A parameter the declaration dropped is dropped here too, and
        // at the same positions: `generic_elision` is the one answer
        // both sides read (see that module's docs for the rule).
        let elision = self.context.type_param_elision();
        if args.is_empty() {
            // A generic class referenced without resolved type
            // arguments (e.g. a local temp typed as bare
            // `ImmutableCache` for a `new ImmutableCache<T>()` result)
            // still needs its generic slots spelled so the reference is
            // well-formed. Emit inference placeholders `<_, _>` per
            // declared type parameter and let Rust unify them from the
            // initializer (was E0107 in the generated `ImmutableCache`).
            let declared_params = crate::classes::find_class(self.mir, name)
                .map_or(0, |class| class.type_params.len());
            let declared_params = elision
                .retain_carried(name, (0..declared_params).collect::<Vec<_>>())
                .len();
            if declared_params == 0 {
                Ok(RustType::raw(type_name))
            } else {
                let placeholders = vec!["_"; declared_params].join(", ");
                Ok(RustType::with_shape(
                    format!("{type_name}<{placeholders}>"),
                    RustShape::GenericClass,
                ))
            }
        } else {
            // A reference interned before the declaration's defaults were
            // known (an alias lowered ahead of the interface it names)
            // can carry fewer arguments than the struct declares
            // (`TypedResponse<O>` for `TypedResponse<T, U = .., F = ..>`).
            // Every interned type is rendered, so the missing trailing
            // positions take the declared defaults, and the erased
            // carrier where a parameter has none, keeping the arity the
            // struct declares.
            let declared = crate::classes::find_class(self.mir, name)
                .map(|class| class.type_params.clone())
                .or_else(|| {
                    self.mir
                        .interfaces
                        .iter()
                        .find(|interface| interface.name == name)
                        .map(|interface| interface.type_params.clone())
                })
                .unwrap_or_default();
            let mut rendered_args = args
                .iter()
                .map(|arg| self.rust_type(*arg, false, substitution))
                .collect::<Result<Vec<_>, EmitError>>()?;
            for param in declared.iter().skip(args.len()) {
                rendered_args.push(match param.default {
                    Some(default) => self.rust_type(default, false, substitution)?,
                    None => RustType::erased(),
                });
            }
            let positions = (0..rendered_args.len()).collect::<Vec<_>>();
            let carried = elision.retain_carried(name, positions);
            if carried.is_empty() {
                return Ok(RustType::raw(type_name));
            }
            let carried_args = carried
                .iter()
                .filter_map(|index| rendered_args.get(*index))
                .collect::<Vec<_>>();
            let arg_text = carried_args
                .iter()
                .map(|arg| arg.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Ok(
                RustType::with_shape(format!("{type_name}<{arg_text}>"), RustShape::GenericClass)
                    .holding(carried_args.iter().copied()),
            )
        }
    }

    /// A function value.
    fn function_rust_type(
        &self,
        function: &FunctionType,
        allow_impl_trait: bool,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustType, EmitError> {
        if self.is_erased_unknown_rest_function(function) && !function.may_throw {
            return Ok(RustType::with_shape(
                "SmeltErasedFunction",
                RustShape::ErasedFunction,
            ));
        }
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| self.function_type_param(function, index, *param, substitution))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let return_ty = self.function_value_return_type(function, substitution)?;
        let text = if allow_impl_trait {
            format!("impl Fn({params}) -> {return_ty}")
        } else {
            format!("::std::rc::Rc<dyn Fn({params}) -> {return_ty}>")
        };
        let parts = function
            .params
            .iter()
            .chain([&function.return_ty])
            .map(|part| self.rust_type(*part, false, substitution))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RustType::with_shape(text, RustShape::Callable).holding(&parts))
    }
}
