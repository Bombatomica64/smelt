//! Host, global, HTTP, and external-class interop: reading/writing host-overridable globals, HTTP fetch text, and instantiating externally-defined classes.

use super::*;

impl FunctionEmitter<'_> {
    /// The `thread_local!` slot identifier for a host constructor's override
    /// state (`SMELT_HOST_OVERRIDE_<NAME>`).
    fn host_override_slot_ident(&self, class: Symbol) -> Result<String, EmitError> {
        let name = self.symbol_name(class)?;
        Ok(format!(
            "{prefix}{suffix}",
            prefix = smelt_stdlib::runtime_symbols::host_override::SLOT_PREFIX,
            suffix = crate::stdlib::host_override_slot_suffix(name),
        ))
    }

    /// Emit a read of a host constructor's override slot (`globalThis.<class>`).
    ///
    /// The read helper yields a `SmeltUnknown` (native-handle marker for
    /// `Native`, the stored ctor for `Ctor`, `undefined` for `Absent`); the
    /// result is coerced to the destination type.
    pub(super) fn host_global_read_text(&self, class: Symbol, dest_ty: TypeId) -> Result<String, EmitError> {
        let slot = self.host_override_slot_ident(class)?;
        let name = self.symbol_name(class)?.to_owned();
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = format!(
            "{slot}.with(|slot| {read}(slot, {name:?}))",
            read = smelt_stdlib::runtime_symbols::host_override::READ,
        );
        self.value_at_type_text(&call, unknown_ty, dest_ty)
    }

    /// Emit a write to a host constructor's override slot
    /// (`globalThis.<class> = value`). The stored value is erased to
    /// `SmeltUnknown` for classification; the helper returns it so the write
    /// composes as an expression.
    pub(super) fn host_global_write_text(
        &self,
        class: Symbol,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let slot = self.host_override_slot_ident(class)?;
        let erased = self.erase(value)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = format!(
            "{slot}.with(|slot| {write}(slot, {erased}))",
            write = smelt_stdlib::runtime_symbols::host_override::WRITE,
        );
        self.value_at_type_text(&call, unknown_ty, dest_ty)
    }

    /// Emit a presence probe of a host constructor's override slot
    /// (`typeof <class> !== 'undefined'` for a reassigned host name).
    pub(super) fn host_global_present_text(&self, class: Symbol) -> Result<String, EmitError> {
        let slot = self.host_override_slot_ident(class)?;
        Ok(format!(
            "{slot}.with({present})",
            present = smelt_stdlib::runtime_symbols::host_override::PRESENT,
        ))
    }

    /// Converts a set insertion operation to Rust text.
    /// Converts a blocking HTTP GET operation to Rust text.
    pub(super) fn http_get_text(&self, url: &Operand) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(url)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("HTTP GET URL must be a string"));
        }
        Ok(format!(
            "reqwest::blocking::get({}).expect(\"HTTP GET failed\").text().expect(\"HTTP response body read failed\")",
            self.operand_text(url)?
        ))
    }

    /// Emit a read of a mutable global's thread-local cell.
    ///
    /// Copy primitives read through `Cell::get`; strings clone the borrowed
    /// `RefCell<String>` contents.
    pub(super) fn global_get_text(&self, global: u32) -> Result<String, EmitError> {
        let name = crate::global_static_name(self.mir, global);
        let ty = self.global_ty(global)?;
        if matches!(self.mir.types.get(ty), Some(Type::String)) {
            Ok(format!("{name}.with(|value| value.borrow().clone())"))
        } else {
            Ok(format!("{name}.with(::std::cell::Cell::get)"))
        }
    }

    /// Emit a store into a mutable global's thread-local cell.
    ///
    /// The stored value is hoisted to a temporary so the block evaluates to the
    /// stored value, letting `++`/`+=` compose as expressions. Copy primitives
    /// use `Cell::set`; strings replace the `RefCell<String>` contents.
    pub(super) fn global_set_text(&self, global: u32, value: &Operand) -> Result<String, EmitError> {
        let name = crate::global_static_name(self.mir, global);
        let ty = self.global_ty(global)?;
        let value_text = self.value_at_type(value, ty)?;
        if matches!(self.mir.types.get(ty), Some(Type::String)) {
            Ok(format!(
                "{{ let smelt_global_value = {value_text}; {name}.with(|value| *value.borrow_mut() = smelt_global_value.clone()); smelt_global_value }}"
            ))
        } else {
            Ok(format!(
                "{{ let smelt_global_value = {value_text}; {name}.with(|value| value.set(smelt_global_value)); smelt_global_value }}"
            ))
        }
    }

    /// Look up the primitive type of a mutable global by index.
    fn global_ty(&self, global: u32) -> Result<TypeId, EmitError> {
        self.mir
            .globals
            .get(id_index(global, "mutable global index does not fit usize")?)
            .map(|entry| entry.ty)
            .ok_or_else(|| EmitError::new("mutable global index is out of range"))
    }

    /// Emit an erased imported class instance as an unknown object.
    ///
    /// External constructors from packages that are not part of the current
    /// manifest cannot be called directly from generated Rust. The arguments
    /// are still rendered into a discarded tuple so their lowered code remains
    /// type-checked and any future side-effect modeling has a concrete place to
    /// attach before the value is represented as `SmeltUnknown`.
    pub(super) fn external_class_instance_text(
        &self,
        class: Symbol,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let class_name = self.symbol_name(class)?;
        if smelt_stdlib::typescript_stdlib_class(class_name)
            == Some(smelt_stdlib::StdlibClass::RegExp)
        {
            let pattern = args.first().map_or_else(
                || Ok("\"\".to_owned()".to_owned()),
                |arg| self.string_like_operand_text(arg, "RegExp pattern"),
            )?;
            let flags = args.get(1).map_or_else(
                || Ok("\"\".to_owned()".to_owned()),
                |arg| self.string_like_operand_text(arg, "RegExp flags"),
            )?;
            return Ok(format!("SmeltRegExp::new({pattern}, {flags})"));
        }
        let args_text = args
            .iter()
            .map(|arg| self.operand_text(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let args_tuple_text = if args_text.is_empty() {
            "()".to_owned()
        } else {
            format!("({args_text},)")
        };
        Ok(format!(
            "{{ let _smelt_external_args = {args_tuple_text}; let mut _smelt_external = ::std::collections::HashMap::new(); _smelt_external.insert(\"__class\".to_owned(), SmeltUnknown::String({class_name:?}.to_owned())); SmeltUnknown::Object(SmeltObject::new(_smelt_external)) }}"
        ))
    }
}
