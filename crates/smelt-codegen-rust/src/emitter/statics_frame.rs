//! The erased form of a per-evaluation class's statics frame, and the frame
//! identity its instances carry.
//!
//! A class declared inside a function whose static initializers read the
//! function's locals keeps its statics in one *frame* per evaluation of its
//! declaration (`smelt_hir::Class::statics_of`): a reference class whose fields
//! are the static fields and whose methods are the static methods. The frame
//! IS that evaluation's class value. Typed, it is the frame handle; erased, it
//! must be the class value JavaScript code sees, so its erased object carries
//! the class-value markers on top of its fields and methods:
//!
//! - `name` and `__smelt_class_value` (the owner class's source name), so the
//!   runtime treats it as a class (`instanceof`, `typeof`, `.name`);
//! - `__smelt_call`, the constructor: it builds an owner instance of THIS
//!   evaluation, passing the frame as the constructor's hidden leading
//!   argument;
//! - `__smelt_class_fields` / `__smelt_class_methods`, so `Object.keys` and
//!   friends enumerate the static fields (not the methods or `name`), read
//!   live from the frame;
//! - `__smelt_class_frame`, the frame's identity.
//!
//! An erased owner INSTANCE carries `__smelt_class_frame` too (the identity
//! of the frame it was constructed by), and `smelt_instance_of_value` requires
//! the two to match: an instance of one evaluation is not an instance of
//! another evaluation's class, though both are the same Rust struct.

use smelt_hir::{Symbol, Type};
use smelt_stdlib::runtime_symbols::markers::{
    SMELT_CLASS_FIELDS, SMELT_CLASS_FRAME, SMELT_CLASS_METHODS, SMELT_METHOD,
};

use crate::rust::RustExpr;

use super::{EmitError, FunctionEmitter};

impl FunctionEmitter<'_> {
    /// The marker pushes an erased object of class `class` carries for
    /// per-evaluation statics, spelled against the handle `smelt_struct_value`:
    /// the class-value markers when `class` is a statics frame, the frame
    /// identity when it is the owner of one, nothing otherwise.
    pub(super) fn statics_frame_markers(
        &self,
        class: Symbol,
        reference: bool,
    ) -> Result<String, EmitError> {
        if let Some(owner) =
            crate::classes::find_class(self.mir, class).and_then(|found| found.statics_of)
        {
            return self.statics_frame_class_value_markers(class, owner);
        }
        let Some(frame_field) = self.owner_frame_field(class) else {
            return Ok(String::new());
        };
        let field_name = self.context.names().field_text(self.mir, frame_field);
        let frame = if reference {
            format!("smelt_struct_value.0.borrow().{field_name}")
        } else {
            format!("smelt_struct_value.{field_name}")
        };
        Ok(self.carrier_format(self.carrier()?, format_args!(
            "smelt_object_entries.push((\"{SMELT_METHOD}:{SMELT_CLASS_FRAME}\".to_owned(), SmeltUnknown::Number(smelt_reference_object_identity(&{frame}.0) as f64))); "
        )))
    }

    /// `value` (of type `ty`) erased through the coercion seam, for an
    /// erased adapter built outside the emitter (`class_proto`).
    pub(crate) fn erased_value_text(
        &self,
        value: &RustExpr,
        ty: smelt_hir::TypeId,
    ) -> Result<RustExpr, EmitError> {
        self.erase_rendered(value, ty)
    }

    /// Whether `class` is a statics frame, whose erased form is a class value
    /// rather than an instance (it carries no `__smelt_class` marker).
    pub(super) fn is_statics_frame_class(&self, class: Symbol) -> bool {
        crate::classes::find_class(self.mir, class).is_some_and(|found| found.statics_of.is_some())
    }

    /// The hidden field of owner class `class` that holds its statics frame.
    fn owner_frame_field(&self, class: Symbol) -> Option<Symbol> {
        let frame = self
            .mir
            .classes
            .iter()
            .find(|candidate| candidate.statics_of == Some(class))?;
        let found = crate::classes::find_class(self.mir, class)?;
        found
            .fields
            .iter()
            .find(|field| matches!(self.mir.types.get(field.ty), Some(Type::Class { name, .. }) if *name == frame.name))
            .map(|field| field.name)
    }

    /// The class-value markers of frame class `frame` (statics of `owner`).
    fn statics_frame_class_value_markers(
        &self,
        frame: Symbol,
        owner: Symbol,
    ) -> Result<String, EmitError> {
        let carrier = self.carrier()?;
        let owner_name = self.symbol_source_name(owner)?.to_owned();
        // An anonymous class expression (`return class { .. }`) has the empty
        // name in JavaScript; its symbol is minted.
        let display_name = if self.mir.symbols.origin(owner) == smelt_hir::SymbolOrigin::Synthesized
        {
            String::new()
        } else {
            owner_name.clone()
        };
        let frame_class = crate::classes::find_class(self.mir, frame)
            .ok_or_else(|| EmitError::new("statics frame class is missing"))?;
        let mut markers = String::new();
        let fields = frame_class
            .fields
            .iter()
            .filter(|field| field.visibility.is_own_property())
            .map(|field| {
                let key = self.symbol_source_name(field.name)?;
                Ok(format!("SmeltUnknown::Array(vec![SmeltUnknown::String({key:?}.into()), SmeltUnknown::Bool(true)].into())"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let methods = frame_class
            .methods
            .iter()
            .filter_map(|id| self.function_by_id(*id))
            .filter_map(|function| match function.origin {
                smelt_mir::HirOrigin::ClassMethod { method, .. } => Some(method),
                _ => None,
            })
            .map(|method| {
                let key = self.symbol_source_name(method)?;
                Ok(format!("SmeltUnknown::String({key:?}.into())"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        markers.push_str(&self.carrier_format(carrier, format_args!(
            "smelt_object_entries.push((\"name\".to_owned(), SmeltUnknown::String({display_name:?}.into()))); \
             smelt_object_entries.push((\"__smelt_class_value\".to_owned(), SmeltUnknown::String({owner_name:?}.into()))); \
             smelt_object_entries.push((\"{SMELT_CLASS_FIELDS}\".to_owned(), SmeltUnknown::Array(::std::vec::Vec::<SmeltUnknown>::from([{fields}]).into()))); \
             smelt_object_entries.push((\"{SMELT_CLASS_METHODS}\".to_owned(), SmeltUnknown::Array(::std::vec::Vec::<SmeltUnknown>::from([{methods}]).into()))); \
             smelt_object_entries.push((\"{SMELT_METHOD}:{SMELT_CLASS_FRAME}\".to_owned(), SmeltUnknown::Number(smelt_reference_object_identity(&smelt_struct_value.0) as f64))); "
        )));
        markers.push_str(&self.statics_frame_hooks(frame_class)?);
        if let Some(constructor) = self.statics_frame_constructor(owner)? {
            markers.push_str(&format!(
                "smelt_object_entries.push((\"__smelt_call\".to_owned(), {constructor})); ",
                constructor = constructor.delimited()
            ));
        }
        Ok(markers)
    }

    /// The live static-field hooks of the class value of frame `frame_class`
    /// (the handle `smelt_struct_value`): `__smelt_static_get` reads a field
    /// from the frame, `__smelt_static_set` writes one into it, so a read
    /// through the erased class value sees every write, through it or
    /// through the typed frame, as the module-global hooks do for a class
    /// evaluated once (`FunctionEmitter::class_value_static_hooks`).
    fn statics_frame_hooks(&self, frame_class: &smelt_mir::MirClass) -> Result<String, EmitError> {
        let carrier = self.carrier()?;
        let mut get_arms = String::new();
        let mut set_arms = String::new();
        for field in frame_class
            .fields
            .iter()
            .filter(|field| field.visibility.is_own_property())
        {
            let key = self.symbol_source_name(field.name)?.to_owned();
            let field_name = self.context.names().field_text(self.mir, field.name);
            let read = self.erase_rendered(
                &RustExpr::chain(format!("smelt_frame.0.borrow().{field_name}")).cloned_value(),
                field.ty,
            )?;
            get_arms.push_str(&format!("{key:?} => {}, ", read.delimited()));
            let value = self.extract_rendered(
                &RustExpr::path("smelt_static_value"),
                field.ty,
                &self.render_scope(),
            )?;
            set_arms.push_str(&format!(
                "{key:?} => {{ let smelt_stored = {}; smelt_frame.0.borrow_mut().{field_name} = smelt_stored; true }}, ",
                value.delimited()
            ));
        }
        if get_arms.is_empty() {
            return Ok(String::new());
        }
        let key_text = "let smelt_static_key = match smelt_args.first() { Some(SmeltUnknown::String(smelt_key)) => smelt_key.to_string(), _ => return Ok(SmeltUnknown::Undefined) };";
        Ok(self.carrier_format(carrier, format_args!(
            "smelt_object_entries.push((\"__smelt_method:{get_hook}\".to_owned(), {{ let smelt_frame = smelt_struct_value.clone(); SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {{ {key_text} Ok(match smelt_static_key.as_str() {{ {get_arms}_ => SmeltUnknown::Undefined }}) }})) }})); \
             smelt_object_entries.push((\"__smelt_method:{set_hook}\".to_owned(), {{ let smelt_frame = smelt_struct_value.clone(); SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {{ {key_text} let smelt_static_value = smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Undefined); let _ = &smelt_static_value; Ok(SmeltUnknown::Bool(match smelt_static_key.as_str() {{ {set_arms}_ => false }})) }})) }})); ",
            get_hook = smelt_stdlib::runtime_symbols::markers::SMELT_STATIC_GET,
            set_hook = smelt_stdlib::runtime_symbols::markers::SMELT_STATIC_SET,
        )))
    }

    /// The erased constructor of per-evaluation class `owner` for the frame
    /// `smelt_struct_value`: each argument rebuilt from the erased carrier,
    /// the frame passed as the hidden leading argument, and the instance
    /// erased back out as a class instance (with its methods and its frame's
    /// identity). `None` when an argument cannot be rebuilt, or the class is
    /// abstract or generic (no `new` can be spelled for it).
    fn statics_frame_constructor(&self, owner: Symbol) -> Result<Option<RustExpr>, EmitError> {
        let Some(class) = crate::classes::find_class(self.mir, owner) else {
            return Ok(None);
        };
        if !class.type_params.is_empty() || class.is_abstract {
            return Ok(None);
        }
        let (Some(self_ty), Some(function)) = (
            crate::classes::class_self_type(self.mir, owner),
            class.constructor.and_then(|id| self.function_by_id(id)),
        ) else {
            return Ok(None);
        };
        if function.rest.is_some() || function.is_async {
            return Ok(None);
        }
        let mut args = vec!["smelt_frame.clone()".to_owned()];
        for (index, param) in function.params.iter().skip(1).enumerate() {
            let Some(local) = function
                .locals
                .get(usize::try_from(param.0).unwrap_or(usize::MAX))
            else {
                return Ok(None);
            };
            if !super::record_coercion::type_supports_from_unknown(self.mir, self.context, local.ty)
            {
                return Ok(None);
            }
            args.push(
                super::record_coercion::record_field_from_unknown_expr(
                    self.mir,
                    &RustExpr::chain(format!(
                        "smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                    )),
                    local.ty,
                )?
                .into_string(),
            );
        }
        let rust_class = self.context.names().type_name(owner)?.as_str().to_owned();
        let try_operator = if function.can_throw { "?" } else { "" };
        let built = self.erase_rendered(&RustExpr::path("smelt_built"), self_ty)?;
        Ok(Some(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "{{ let smelt_frame = smelt_struct_value.clone(); SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {{ let _ = &smelt_args; let smelt_built = {rust_class}::new({args}){try_operator}; Ok({built}) }})) }}",
            args = args.join(", "),
            built = built.delimited()
        )))))
    }
}
