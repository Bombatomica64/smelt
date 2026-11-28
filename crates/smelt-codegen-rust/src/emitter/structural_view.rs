//! Conversions and property access of structural views
//! (`crate::structural_views`): a record viewed as another record type shares
//! its cell, a cast back checks the cell's concrete record, and a property of
//! a view target is read and written through its trait's accessors.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// The shape name of `ty` when it is a view target: its handle holds
    /// `Rc<dyn <Name>SmeltView>`, so its properties are reached through
    /// accessors and never through `.0.borrow()`.
    pub(super) fn view_target_name(&self, ty: TypeId) -> Option<Symbol> {
        match self.mir.types.get(ty) {
            Some(Type::Class { name, args })
                if args.is_empty() && self.context.structural_views().is_target(*name) =>
            {
                Some(*name)
            }
            _ => None,
        }
    }

    /// The accessor pair of `field` when `ty` is a polymorphic class of a
    /// multiple-inheritance hierarchy, whose cell lends a field view
    /// (`crate::class_polymorphism::lends_field_view`).
    ///
    /// The pair is spelled fully qualified (`<Owner>SmeltFields::count`), so
    /// the call resolves on the receiver's concrete record as well (an
    /// inherited body emitted on a subclass reads its own flat record, which
    /// implements every view of its MRO).
    pub(super) fn field_view_accessors(&self, ty: TypeId, field: Symbol) -> Option<(String, String)> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        let polymorphic = self.context.polymorphic_classes();
        if !crate::class_polymorphism::lends_field_view(self.mir, polymorphic, *name) {
            return None;
        }
        let owner =
            crate::class_polymorphism::field_view_owner(self.mir, polymorphic, *name, field)?;
        let owner_trait = crate::class_polymorphism::fields_trait_name(
            self.context.names().type_name(owner).ok()?.as_str(),
        );
        let (get, get_mut) = crate::class_polymorphism::field_accessor_names(
            &self.context.names().field_text(self.mir, field),
        );
        Some((
            format!("{owner_trait}::{get}"),
            format!("{owner_trait}::{get_mut}"),
        ))
    }

    /// Whether `ty` is a view target (see [`Self::view_target_name`]).
    pub(super) fn is_view_target_type(&self, ty: TypeId) -> bool {
        self.view_target_name(ty).is_some()
    }

    /// The accessor names of the property `field` of the view target `owner`.
    fn view_accessors(
        &self,
        owner: Symbol,
        field: Symbol,
    ) -> Result<crate::structural_views::AccessorNames, EmitError> {
        let shape = crate::structural_views::is_viewable_shape(self.mir, owner)
            .ok_or_else(|| EmitError::new("a view target is not an object shape"))?;
        let fields = crate::classes::effective_interface_fields(self.mir, shape);
        crate::structural_views::accessor_names(self.mir, self.context, &fields)
            .remove(&field)
            .ok_or_else(|| EmitError::new("a view target does not declare the property read"))
    }

    /// The place of the declared property `field` inside the reference
    /// record `base` (a handle of type `base_ty`), for reading.
    ///
    /// `base.0.borrow().field` for a record that owns its cell; for a view
    /// target, the property lent by whichever record the object was built as
    /// (`(*base.0.field())`). Either way the borrow is a temporary of the
    /// enclosing statement.
    pub(super) fn reference_field_read_place(
        &self,
        base: &str,
        base_ty: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        if let Some(owner) = self.view_target_name(base_ty) {
            let accessors = self.view_accessors(owner, field)?;
            return Ok(format!("(*{base}.0.{}())", accessors.get));
        }
        if let Some((get, _)) = self.field_view_accessors(base_ty, field) {
            return Ok(format!("(*{get}(&*{base}.0.borrow()))"));
        }
        Ok(format!(
            "{base}.0.borrow().{}",
            self.context.names().field_text(self.mir, field)
        ))
    }

    /// [`Self::reference_field_read_place`] over a rendered receiver, as an
    /// expression: for a record that owns its cell, exactly the postfix
    /// chain the emitter always built (`base.0.borrow().field`).
    pub(super) fn reference_field_read_expr(
        &self,
        base: &RustExpr,
        base_ty: TypeId,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        if self.is_view_target_type(base_ty) || self.field_view_accessors(base_ty, field).is_some() {
            return Ok(RustExpr::chain(self.reference_field_read_place(
                &base.tight(),
                base_ty,
                field,
            )?));
        }
        Ok(base.postfix(format!(
            ".0.borrow().{}",
            self.context.names().field_text(self.mir, field)
        )))
    }

    /// [`Self::reference_field_read_place`] for writing:
    /// `base.0.borrow_mut().field`, or `(*base.0.field_mut())` on a view
    /// target.
    pub(super) fn reference_field_write_place(
        &self,
        base: &str,
        base_ty: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        if let Some(owner) = self.view_target_name(base_ty) {
            let accessors = self.view_accessors(owner, field)?;
            return Ok(format!("(*{base}.0.{}())", accessors.get_mut));
        }
        if let Some((_, get_mut)) = self.field_view_accessors(base_ty, field) {
            return Ok(format!("(*{get_mut}(&mut *{base}.0.borrow_mut()))"));
        }
        Ok(format!(
            "{base}.0.borrow_mut().{}",
            self.context.names().field_text(self.mir, field)
        ))
    }

    /// The place of declared property `field` of the reference handle
    /// rendered as `base_text`, which reads the local `local` (of type
    /// `base_ty`), for reading.
    ///
    /// A view target lends the property through its trait
    /// ([`Self::reference_field_read_place`]); a handle of a widened generic
    /// class that may be a converting view reads through its accessor guard
    /// ([`Self::reference_field_read_suffix`]); any other record borrows its
    /// own cell. The two view kinds never meet: a view target is a
    /// non-generic object shape, a widened class a generic class.
    pub(super) fn reference_field_read_place_at(
        &self,
        base_text: &str,
        local: LocalId,
        base_ty: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        if self.is_view_target_type(base_ty) || self.field_view_accessors(base_ty, field).is_some() {
            return self.reference_field_read_place(base_text, base_ty, field);
        }
        Ok(format!(
            "{base_text}{}",
            self.reference_field_read_suffix(local, base_ty, field)
        ))
    }

    /// [`Self::reference_field_read_place_at`] over a rendered receiver, as an
    /// expression.
    pub(super) fn reference_field_read_expr_at(
        &self,
        base: &RustExpr,
        local: LocalId,
        base_ty: TypeId,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        if self.is_view_target_type(base_ty) || self.field_view_accessors(base_ty, field).is_some() {
            return self.reference_field_read_expr(base, base_ty, field);
        }
        Ok(base.postfix(self.reference_field_read_suffix(local, base_ty, field)))
    }

    /// [`Self::reference_field_read_place_at`] for writing.
    pub(super) fn reference_field_write_place_at(
        &self,
        base_text: &str,
        local: LocalId,
        base_ty: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        if self.is_view_target_type(base_ty) || self.field_view_accessors(base_ty, field).is_some() {
            return self.reference_field_write_place(base_text, base_ty, field);
        }
        Ok(self.reference_field_write_text(base_text, local, base_ty, field))
    }

    /// The two distinct object shapes of a record-to-record conversion.
    fn shape_pair(&self, source: TypeId, target: TypeId) -> Option<(Symbol, Symbol)> {
        let (
            Some(Type::Class {
                name: from,
                args: from_args,
            }),
            Some(Type::Class {
                name: to,
                args: to_args,
            }),
        ) = (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return None;
        };
        (from != to && from_args.is_empty() && to_args.is_empty()).then_some((*from, *to))
    }

    /// `value` (of shape type `source`) viewed as the view target `target`:
    /// the same `Rc`, unsized into the target's trait object, or re-viewed
    /// through the source trait's conversion method. `None` when `source` is
    /// not planned to convert to `target` in place
    /// (`StructuralViews::converts`).
    ///
    /// Behavior: the result is the same object as `value` (identity, writes
    /// both ways, own keys). Type preservation: the result has `target`'s
    /// handle type, `value` keeps its own, and nothing is erased.
    pub(super) fn structural_view_expr(
        &self,
        value: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some((from, to)) = self.shape_pair(source, target) else {
            return Ok(None);
        };
        let views = self.context.structural_views();
        if !views.converts(from, to) {
            return Ok(None);
        }
        if std::env::var_os("SMELT_STRUCT_VIEW_TRACE").is_some() {
            let name = |symbol: Symbol| self.mir.symbols.get(symbol).unwrap_or("?").to_owned();
            #[expect(
                clippy::print_stderr,
                reason = "SMELT_STRUCT_VIEW_TRACE is an opt-in developer trace"
            )]
            {
                eprintln!("struct-view share {} -> {}", name(from), name(to));
            }
        }
        Ok(Some(RustExpr::atom(
            crate::structural_views::upcast_handle_expr(self.context, from, to, &value.tight())?,
        )))
    }

    /// With `SMELT_STRUCT_VIEW_TRACE` set, print one line to stderr for the
    /// record-to-record conversion `source -> target` the adapter is about to
    /// emit as a field-wise copy, naming why it is not a view. A measurement
    /// aid, the counterpart of `SMELT_LITERAL_RECORD_TRACE`: it is how the
    /// conversions that still copy are counted per cause.
    pub(super) fn trace_record_copy(&self, source: TypeId, target: TypeId) {
        if std::env::var_os("SMELT_STRUCT_VIEW_TRACE").is_none() {
            return;
        }
        let (
            Some(Type::Class {
                name: from,
                args: from_args,
            }),
            Some(Type::Class {
                name: to,
                args: to_args,
            }),
        ) = (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return;
        };
        let class = |name: Symbol| crate::classes::find_class(self.mir, name).is_some();
        let reason = if from == to {
            "same-record-other-arguments"
        } else if class(*to) {
            "class-target"
        } else if class(*from)
            && crate::structural_views::view_source_fields(self.mir, *from).is_none()
        {
            "class-not-viewable"
        } else if !from_args.is_empty() || !to_args.is_empty() {
            "generic"
        } else if crate::structural_views::view_source_fields(self.mir, *from).is_none()
            || crate::structural_views::is_viewable_shape(self.mir, *to).is_none()
        {
            "callable-accessor-or-index-shape"
        } else if crate::structural_views::shares(self.mir, *from, *to) {
            "shares-but-no-flow-seen-or-subclass-redeclares"
        } else {
            let held =
                crate::structural_views::view_source_fields(self.mir, *from).unwrap_or_default();
            let declared = crate::structural_views::is_viewable_shape(self.mir, *to)
                .map(|shape| crate::classes::effective_interface_fields(self.mir, shape))
                .unwrap_or_default();
            if declared
                .iter()
                .all(|wanted| held.iter().any(|field| field.name == wanted.name))
            {
                "property-at-another-type"
            } else {
                "property-the-source-lacks"
            }
        };
        let name = |symbol: Symbol| self.mir.symbols.get(symbol).unwrap_or("?").to_owned();
        #[expect(
            clippy::print_stderr,
            reason = "SMELT_STRUCT_VIEW_TRACE is an opt-in developer trace"
        )]
        {
            eprintln!("struct-view copy {} -> {} {reason}", name(*from), name(*to));
        }
    }

    /// The checked cast of the handle bound to `smelt_struct_value` (of view
    /// target type `source`) to the shape `target`: an `Option` holding the
    /// same object when its cell is a record `target`'s handle can hold.
    /// `None` (no expression) when `source` is not a view target or no cell
    /// it can hold is one of `target`'s.
    ///
    /// The cells tried are the ones both types can hold: `target`'s own cell
    /// and, when `target` is a view target too, each shape that implements
    /// both traits.
    pub(super) fn structural_view_downcast_expr(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some((from, to)) = self.shape_pair(source, target) else {
            return Ok(None);
        };
        let views = self.context.structural_views();
        if !views.is_target(from) || !self.context.is_reference_class(to) {
            return Ok(None);
        }
        // A polymorphic class handle holds the cell of any concrete class of
        // its hierarchy (`crate::class_polymorphism`).
        let polymorphic = self.context.polymorphic_classes();
        let hierarchy = |handle: Symbol| {
            if polymorphic.contains(&handle) {
                crate::class_polymorphism::concrete_classes(self.mir, polymorphic, handle)
            } else {
                Vec::new()
            }
        };
        let (from_cells, to_cells) = (hierarchy(from), hierarchy(to));
        let holds = |handle: Symbol, cell: Symbol| {
            handle == cell
                || views.converts(cell, handle)
                || (handle == from && from_cells.contains(&cell))
                || (handle == to && to_cells.contains(&cell))
        };
        let target_type = self.type_rust_name(to)?;
        let mut cells = Vec::new();
        for candidate in std::iter::once(to)
            .chain(std::iter::once(from))
            .chain(views.implementors(from).iter().copied())
        {
            if holds(from, candidate) && holds(to, candidate) && !cells.contains(&candidate) {
                cells.push(candidate);
            }
        }
        if cells.is_empty() {
            return Ok(None);
        }
        let mut text = "None".to_owned();
        for cell in cells.iter().rev() {
            let cell_type = self.type_rust_name(*cell)?;
            text = format!(
                "match ::std::rc::Rc::clone(&smelt_view_any).downcast::<::std::cell::RefCell<{cell_type}Inner>>() {{ Ok(smelt_cell) => Some({target_type}(smelt_cell)), Err(_) => {text} }}"
            );
        }
        Ok(Some(format!(
            "{{ let smelt_view_any = ::std::rc::Rc::clone(&smelt_struct_value.0).smelt_any(); {text} }}"
        )))
    }
}
