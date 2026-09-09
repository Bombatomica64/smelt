//! Rust emission for the concrete typed-array family.
//!
//! Two rvalues live here: [`ModuleEmitter::typed_array_new_text`], which turns
//! a source `new Uint8Array(..)` / `new ArrayBuffer(n)` into a concrete
//! constructor call, and [`ModuleEmitter::byte_array_op_text`]'s method arms
//! (in `text_codec`), which render a member as the spec member of the same
//! name on the generated `SmeltTypedArray` / `SmeltArrayBuffer`.
//!
//! ## Why the constructor dispatches on argument TYPE
//!
//! `new Uint8Array(x)` is five different operations in JavaScript, chosen by
//! what `x` is: a length, an element list, a re-view over an existing
//! `ArrayBuffer`'s storage, an element-by-element conversion from another view,
//! or the array-like/iterable protocol. A hand-writing Rust team would pick the
//! right constructor at the call site from the static type — and that is what
//! this does. Only when the argument's type carries no shape (an `unknown`, a
//! generic `T`, a union) does the choice move to run time, through the
//! prelude's single `from_erased` boundary; every other spelling resolves
//! statically and shares storage or copies exactly as the spec says.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit a `new <view>(..)` / `new ArrayBuffer(..)` construction.
    ///
    /// `class_name` is the source spelling. It selects the half of the family
    /// (storage for `ArrayBuffer`, an element view for the eleven) and, for a
    /// view, the element kind — which is the only thing the eleven spellings
    /// differ by, since they share one Rust type.
    pub(super) fn typed_array_new_text(
        &self,
        class_name: &str,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        // `new DataView(buffer, byteOffset?, byteLength?)` is the family's one
        // constructor with no element kind at all: the kind arrives with each
        // accessor instead, so the construction takes a window and nothing
        // more. It is asked before the kind lookup because it has none.
        if class_name == "DataView" {
            return self.data_view_new_text(args);
        }
        let Some(kind) = crate::typed_array_prelude::kind_expression(class_name) else {
            return self.array_buffer_new_text(class_name, args);
        };
        let Some(first) = args.first() else {
            return Ok(format!("SmeltTypedArray::with_length({kind}, 0)"));
        };
        // `new View(buffer, byteOffset?, length?)`. Only the storage half takes
        // more than one argument, so a second argument is what identifies the
        // shared-buffer spelling even before the first argument's type is read.
        if args.len() > 1 || self.operand_is_array_buffer(first)? {
            return self.typed_array_over_buffer_text(&kind, args);
        }
        let first_ty = self.operand_ty(first)?;
        let first_text = self.operand_text(first)?;
        match self.mir.types.get(first_ty) {
            // A LENGTH, in elements: the storage is `length * BYTES_PER_ELEMENT`
            // bytes of zero.
            Some(Type::Int | Type::Float) => Ok(format!(
                "SmeltTypedArray::with_length({kind}, (({first_text}) as f64).max(0.0) as usize)"
            )),
            // An element list converts per element at the view's own width and
            // signedness, so `new Uint8Array([256])` is `0` and
            // `new Int8Array([200])` is `-56`.
            Some(Type::List(item_ty)) if self.is_numeric_element_type(*item_ty) => Ok(format!(
                "SmeltTypedArray::with_elements({kind}, &{first_text}.to_vec())"
            )),
            // Another VIEW converts element by element too — not byte by byte —
            // which is why `new Uint8Array(new Float64Array([1]))` is `[1]` and
            // not the eight bytes of a double.
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::TypedArray) =>
            {
                Ok(format!(
                    "SmeltTypedArray::with_elements({kind}, &{first_text}.to_elements())"
                ))
            }
            // The dynamic boundary: an argument whose type carries no shape.
            // One prelude helper decides, rather than an inlined match here, so
            // every erased spelling answers the same way.
            _ => {
                let erased = self.erase_value_text(&first_text, first_ty)?;
                Ok(format!(
                    "SmeltTypedArray::from_erased({kind}, &{erased})"
                ))
            }
        }
    }

    /// Emit `new View(buffer, byteOffset?, length?)`: a view SHARING storage.
    ///
    /// The offset and length are element/byte counts the spec clamps, which the
    /// prelude's `over_buffer` does; a missing length means "to the end of the
    /// buffer", which is what `None` says.
    fn typed_array_over_buffer_text(
        &self,
        kind: &str,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let buffer = args
            .first()
            .ok_or_else(|| EmitError::new("internal: a buffer view needs its storage"))?;
        let buffer_text = self.array_buffer_operand_text(buffer)?;
        let offset_text = match args.get(1) {
            Some(offset) => format!("(({}) as f64).max(0.0) as usize", self.numeric_operand_text(offset)?),
            None => "0".to_owned(),
        };
        let length_text = match args.get(2) {
            Some(length) => format!(
                "Some(((({})) as f64).max(0.0) as usize)",
                self.numeric_operand_text(length)?
            ),
            None => "None".to_owned(),
        };
        Ok(format!(
            "SmeltTypedArray::over_buffer({kind}, &{buffer_text}, {offset_text}, {length_text})"
        ))
    }

    /// Emit `new DataView(buffer, byteOffset?, byteLength?)`.
    ///
    /// The window's bounds are byte counts the spec clamps, which the prelude's
    /// `over_buffer` does; a missing length means "to the end of the buffer",
    /// which is what `None` says. Unlike a typed array's, both bounds are BYTE
    /// counts here, since a `DataView` has no element width of its own.
    fn data_view_new_text(&self, args: &[Operand]) -> Result<String, EmitError> {
        let Some(buffer) = args.first() else {
            return Ok("SmeltDataView::new()".to_owned());
        };
        let buffer_text = self.array_buffer_operand_text(buffer)?;
        let offset_text = match args.get(1) {
            Some(offset) => format!(
                "(({}) as f64).max(0.0) as usize",
                self.numeric_operand_text(offset)?
            ),
            None => "0".to_owned(),
        };
        let length_text = match args.get(2) {
            Some(length) => format!(
                "Some(((({})) as f64).max(0.0) as usize)",
                self.numeric_operand_text(length)?
            ),
            None => "None".to_owned(),
        };
        Ok(format!(
            "SmeltDataView::over_buffer(&{buffer_text}, {offset_text}, {length_text})"
        ))
    }

    /// Emit `new ArrayBuffer(byteLength?)`.
    ///
    /// The storage half interprets no bytes, so its only argument is a byte
    /// count. A name that is neither a view nor `ArrayBuffer` cannot reach
    /// here: the frontend only builds this node for the twelve registered
    /// spellings, so anything else is a lowering bug rather than a source
    /// error.
    fn array_buffer_new_text(
        &self,
        class_name: &str,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        // `SharedArrayBuffer` is the same storage with the species flag set:
        // one constructor argument, one byte count, and a tag and `instanceof`
        // answer that differ. `new SharedArrayBuffer(n, { maxByteLength })` —
        // the growable form — is NOT modeled: `grow`/`growable`/`maxByteLength`
        // have no member here, so a program that uses them reports the missing
        // member rather than silently answering a fixed length.
        let constructor = match class_name {
            "ArrayBuffer" => "new",
            "SharedArrayBuffer" => "new_shared",
            _ => {
                return Err(EmitError::new(format!(
                    "internal: `{class_name}` is not a typed-array family constructor"
                )));
            }
        };
        let byte_length = match args.first() {
            Some(argument) => format!("(({}) as f64).max(0.0) as usize", self.numeric_operand_text(argument)?),
            None => "0".to_owned(),
        };
        Ok(format!("SmeltArrayBuffer::{constructor}({byte_length})"))
    }

    /// Emit a `DataView` accessor call: `getInt16(0)`, `setFloat64(0, x, true)`.
    ///
    /// A FALLIBLE builtin call, so the text ends in `?`: an offset outside the
    /// view is a catchable `RangeError`, and only a call carries the unwind
    /// edge that reaches a `try` (see
    /// `blocker-logs/standards-throwing-rvalue.md`). The adapter owns the
    /// throw; the prelude's checked pair owns the bound.
    ///
    /// The element kind and the direction arrive already resolved, on the
    /// builtin itself — the registry read the accessor's NAME once, at the
    /// frontend dispatch — so a `getUint8Clamped`, which JavaScript does not
    /// define, cannot reach here at all.
    ///
    /// `args` is the call's own list: the receiver first, then the accessor's
    /// arguments as written. The byte-order argument defaults to FALSE, the
    /// opposite of a typed array's fixed little-endian order; that default is
    /// the spec's, and it is the one thing about `DataView` most easily got
    /// wrong.
    pub(super) fn data_view_access_text(
        &self,
        write: bool,
        element: smelt_stdlib::TypedArrayElement,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let Some((view, accessor_args)) = args.split_first() else {
            return Err(EmitError::new(
                "internal: a `DataView` accessor call takes its receiver first",
            ));
        };
        let kind = crate::typed_array_prelude::element_kind_expression(element);
        let view_text = self.operand_text(view)?;
        let offset = match accessor_args.first() {
            Some(offset) => format!(
                "(({}) as f64).max(0.0) as usize",
                self.numeric_operand_text(offset)?
            ),
            None => "0".to_owned(),
        };
        // A write takes the value between the offset and the flag, so the flag
        // is the third argument for `set` and the second for `get`.
        let (value, endian_index) = if write {
            let value = match accessor_args.get(1) {
                Some(value) => self.numeric_operand_text(value)?,
                None => "0.0".to_owned(),
            };
            (Some(value), 2)
        } else {
            (None, 1)
        };
        let little_endian = match accessor_args.get(endian_index) {
            Some(flag) => self.truthy_operand_text(flag)?,
            None => "false".to_owned(),
        };
        // The trailing `?` is what marks the call fallible to
        // `emit_throwing_call_terminator`, which renders the shape that binds
        // the caught `RangeError` and jumps to the handler's catch block.
        Ok(match value {
            Some(value) => format!(
                "{adapter}(&{view_text}, {kind}, {offset}, {value}, {little_endian})?",
                adapter = crate::thrown::DATA_VIEW_SET_FN,
            ),
            None => format!(
                "{adapter}(&{view_text}, {kind}, {offset}, {little_endian})?",
                adapter = crate::thrown::DATA_VIEW_GET_FN,
            ),
        })
    }

    /// Render an operand as an `SmeltArrayBuffer`.
    ///
    /// Already-concrete storage passes through; anything else enters through
    /// the storage type's own `SmeltFromUnknown` boundary adapter, which is the
    /// same route `TextDecoder.decode`'s byte argument takes.
    fn array_buffer_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(operand)?;
        if self.operand_is_array_buffer(operand)? {
            return Ok(text);
        }
        let ty = self.operand_ty(operand)?;
        let erased = self.erase_value_text(&text, ty)?;
        Ok(format!("SmeltArrayBuffer::smelt_from_unknown({erased})"))
    }

    /// Render an operand as an `f64`, coercing an erased one at the boundary.
    pub(super) fn numeric_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(operand)?;
        let text = self.operand_text(operand)?;
        let float_ty = self.type_id(Type::Float)?;
        self.value_at_type_text(&text, ty, float_ty)
    }

    /// Emit an indexed element READ on a typed-array view, if the base is one.
    ///
    /// `None` when the base is not a view, so the caller falls through to its
    /// own arms. The element is decoded at the view's width; a missing one
    /// answers the same zero a typed list's missing element does, because both
    /// are reads of a concrete numeric collection whose Rust type has no
    /// `undefined`.
    pub(super) fn typed_array_index_read_text(
        &self,
        base_ty: TypeId,
        base: smelt_mir::LocalId,
        index: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !self.is_typed_array_view_class_type(base_ty)? {
            return Ok(None);
        }
        let base_text = self.local_value_text(base)?;
        let index_text = self.numeric_operand_text(index)?;
        Ok(Some(format!(
            "{base_text}.get({index_text}).unwrap_or(0.0)"
        )))
    }

    /// Emit an indexed element WRITE on a typed-array view, if the base is one.
    ///
    /// `None` when the base is not a view. The value is encoded at the view's
    /// width and signedness, so `view[0] = 256` on a `Uint8Array` stores `0`
    /// and on a `Uint8ClampedArray` stores `255`.
    pub(super) fn typed_array_index_write_statement(
        &self,
        base_ty: TypeId,
        base: smelt_mir::LocalId,
        index: &Operand,
        value: &smelt_mir::Rvalue,
    ) -> Result<Option<String>, EmitError> {
        if !self.is_typed_array_view_class_type(base_ty)? {
            return Ok(None);
        }
        let float_ty = self.type_id(Type::Float)?;
        let rendered_value = self.rvalue_text_for_dest(value, float_ty)?;
        let base_text = self.local_value_text(base)?;
        let index_text = self.numeric_operand_text(index)?;
        Ok(Some(format!(
            "{base_text}.set_index({index_text}, {rendered_value});"
        )))
    }

    /// Emit `instanceof` for a receiver that is already a concrete family value.
    ///
    /// The two halves are never instances of each other, and a VIEW's identity
    /// is its element kind — a runtime field — so `view instanceof Uint8Array`
    /// is a class-name comparison rather than the marker probe an erased value
    /// needs. Every other class (`DataView`, `SharedArrayBuffer`, a user class)
    /// is statically false: a concrete family value cannot be one.
    pub(super) fn typed_array_instance_of_text(
        &self,
        value: &Operand,
        class_name: &str,
    ) -> Result<Option<String>, EmitError> {
        let value_ty = self.operand_ty(value)?;
        if self.is_typed_array_view_class_type(value_ty)? {
            if smelt_stdlib::is_typed_array_class_name(class_name) {
                let text = self.operand_text(value)?;
                return Ok(Some(format!("{text}.class_name() == {class_name:?}")));
            }
            return Ok(Some("false".to_owned()));
        }
        if self.operand_is_array_buffer(value)? {
            // `ArrayBuffer` and `SharedArrayBuffer` are unrelated constructors
            // in JavaScript, so one storage type answering both would be
            // wrong: the species flag is what separates them, and every other
            // class answers false.
            let text = self.operand_text(value)?;
            return Ok(Some(match class_name {
                "ArrayBuffer" => format!("!{text}.is_shared()"),
                "SharedArrayBuffer" => format!("{text}.is_shared()"),
                _ => "false".to_owned(),
            }));
        }
        Ok(None)
    }

    /// Return whether a type is a concrete typed-array VIEW class.
    pub(super) fn is_typed_array_view_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::TypedArray))
    }

    /// Return whether an operand's static type is the modeled `ArrayBuffer`.
    pub(super) fn operand_is_array_buffer(&self, operand: &Operand) -> Result<bool, EmitError> {
        let ty = self.operand_ty(operand)?;
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::ArrayBuffer))
    }

    /// The `TypeId` of the modeled `ArrayBuffer` class, as `.buffer` answers it.
    ///
    /// Looked up in the type table rather than interned: a MIR type table is
    /// read-only at emit time, and a program that reads `.buffer` always has
    /// the class in its table because the read itself is typed by the frontend.
    pub(super) fn array_buffer_type_id(&self) -> Result<TypeId, EmitError> {
        for candidate in self.mir.types.all() {
            if let Type::Class { name, .. } = candidate
                && self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::ArrayBuffer)
            {
                return self.type_id(candidate.clone());
            }
        }
        Err(EmitError::new(
            "internal: a typed-array `.buffer` read has no `ArrayBuffer` type",
        ))
    }

    /// Return whether a list's element type is a plain number.
    ///
    /// `new Uint8Array([1, 2])` reads its elements straight out of an
    /// `SmeltList<f64>`; a list of anything else has to go through the erased
    /// boundary, because the spec converts each element with `ToNumber`.
    fn is_numeric_element_type(&self, item_ty: TypeId) -> bool {
        matches!(self.mir.types.get(item_ty), Some(Type::Int | Type::Float))
    }
}
