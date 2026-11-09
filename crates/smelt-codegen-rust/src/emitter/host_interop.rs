//! Host, global, HTTP, and external-class interop: reading/writing host-overridable globals, HTTP fetch text, and instantiating externally-defined classes.

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Name the source site of the function currently being emitted.
    ///
    /// An `EmitError` raised deep in emission used to identify itself only by
    /// the enclosing function's NAME, which is `<unnamed>` for every closure.
    /// A blocker inside a callback therefore said nothing about where it was,
    /// and pinning one in a corpus the size of Hono meant bisecting the
    /// manifest's `exclude` list one directory at a time.
    ///
    /// MIR is flattened and carries no per-statement spans, but the entry
    /// BLOCK has one, and `Mir::file_paths` preserves `FileId -> path`, so a
    /// file and byte offset are recoverable. That is block granularity, not
    /// statement granularity -- enough to name the function in the source,
    /// which is what a reader needs to find it.
    pub(super) fn current_function_site(&self) -> String {
        let name = self.symbol_name(self.function.name).unwrap_or("<unnamed>");
        let Some(block) = self.function.blocks.first() else {
            return format!("`{name}`");
        };
        let span = block.span;
        match self.mir.file_paths.get(&span.file) {
            Some(path) => format!("`{name}` at {path}:{}..{}", span.start, span.end),
            // A synthesized function (a generic instantiation, a closure the
            // emitter built) has no source file of its own.
            None => format!("`{name}` at byte {}..{}", span.start, span.end),
        }
    }

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
    pub(super) fn host_global_read_expr(&self, class: Symbol, dest_ty: TypeId) -> Result<RustExpr, EmitError> {
        let slot = self.host_override_slot_ident(class)?;
        let name = self.symbol_name(class)?.to_owned();
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = RustExpr::chain(format!(
            "{slot}.with(|smelt_slot| {read}(smelt_slot, {name:?}))",
            read = smelt_stdlib::runtime_symbols::host_override::READ,
        ));
        self.rendered_at_type(&call, unknown_ty, dest_ty, &self.render_scope())
    }

    /// Emit a write to a host constructor's override slot
    /// (`globalThis.<class> = value`). The stored value is erased to
    /// `SmeltUnknown` for classification; the helper returns it so the write
    /// composes as an expression.
    pub(super) fn host_global_write_expr(
        &self,
        class: Symbol,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let slot = self.host_override_slot_ident(class)?;
        let erased = self.erase(value)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = RustExpr::chain(format!(
            "{slot}.with(|smelt_slot| {write}(smelt_slot, {erased}))",
            write = smelt_stdlib::runtime_symbols::host_override::WRITE, erased = erased.delimited(),
        ));
        self.rendered_at_type(&call, unknown_ty, dest_ty, &self.render_scope())
    }

    /// Emit `process.argv`: the executable path, twice, then the arguments
    /// after the program name (see `Rvalue::ProcessArgv`), as a `Vec` the
    /// fresh-list conversion wraps. The executable path is absolute, as
    /// Node's `argv[0]` and `argv[1]` are, falling back to the name the
    /// program was started under when the platform cannot say.
    pub(super) fn process_argv_expr(&self) -> RustExpr {
        let element = if self.indexes_utf16() { "JsString::from(smelt_arg.as_str())" } else { "smelt_arg" };
        RustExpr::from(format!(
            "{{ let mut smelt_args = ::std::env::args(); let smelt_started = smelt_args.next().unwrap_or_default(); let smelt_exe = ::std::env::current_exe().map(|smelt_path| smelt_path.to_string_lossy().into_owned()).unwrap_or(smelt_started); ::std::iter::once(smelt_exe.clone()).chain(::std::iter::once(smelt_exe)).chain(smelt_args).map(|smelt_arg| {element}).collect::<Vec<_>>() }}"
        ))
    }

    /// Emit a presence probe of a host constructor's override slot
    /// (`typeof <class> !== 'undefined'` for a reassigned host name).
    pub(super) fn host_global_present_expr(&self, class: Symbol) -> Result<RustExpr, EmitError> {
        let slot = self.host_override_slot_ident(class)?;
        Ok(RustExpr::chain(format!(
            "{slot}.with({present})",
            present = smelt_stdlib::runtime_symbols::host_override::PRESENT,
        )))
    }

    /// Emit `new <HostClass>(args...)` for a registry host object as a call of
    /// the shared reflected constructor.
    ///
    /// The direct `new DataView(buf, 1, 2)` spelling and the reflected
    /// `new Object.getPrototypeOf(view).constructor(...)` spelling must produce
    /// indistinguishable records — es-toolkit's `clone` uses the reflected form
    /// where `cloneDeepWith` uses the direct one, and their results are compared
    /// against each other. Emitting both as `smelt_reflected_construct(kind, ..)`
    /// makes that identity hold by construction rather than by keeping two
    /// lowerings in sync.
    ///
    /// The *kind* string is the class's registry marker with its `__smelt_`
    /// prefix stripped (`"dataview"` for `__smelt_dataview`), which is the key
    /// the emitted `smelt_reflected_construct` dispatches on.
    pub(super) fn host_construct_expr(
        &self,
        class_name: &str,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(kind) = crate::reflection_prelude::reflected_construct_kind(class_name) else {
            return Err(EmitError::new(format!(
                "`{class_name}` is not a modeled host constructor"
            )));
        };
        let arg_texts = args
            .iter()
            .map(|arg| self.erase(arg))
            .collect::<Result<Vec<_>, _>>()?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = RustExpr::chain(format!(
            "{construct}({kind:?}, vec![{args}])",
            construct = smelt_stdlib::runtime_symbols::host::REFLECTED_CONSTRUCT,
            args = arg_texts.join(", "),
        ));
        self.rendered_at_type(&call, unknown_ty, dest_ty, &self.render_scope())
    }

    /// Emit the interned value for a global builtin name used as a value.
    ///
    /// One record per name for the whole program, so `Blob === Blob` holds and a
    /// host record's `.constructor` can answer with the very same object.
    pub(super) fn builtin_namespace_expr(
        &self,
        name: &str,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = RustExpr::chain(format!(
            "{namespace}({name:?})",
            namespace = smelt_stdlib::runtime_symbols::host::BUILTIN_NAMESPACE,
        ));
        self.rendered_at_type(&call, unknown_ty, dest_ty, &self.render_scope())
    }

    /// Emit the enclosing function's `arguments` object from its parameters.
    ///
    /// The positional parameters become the leading elements and the rest
    /// parameter's list is flattened onto the end, which is how the runtime helper
    /// recovers the original call's argument vector.
    pub(super) fn arguments_object_expr(
        &self,
        fixed: &[Operand],
        rest: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let fixed_texts = fixed
            .iter()
            .map(|value| self.erase(value))
            .collect::<Result<Vec<_>, _>>()?;
        let rest_text = match rest {
            Some(operand) => format!("Some({})", self.erase(operand)?.delimited()),
            None => "None".to_owned(),
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = RustExpr::chain(format!(
            "{helper}(vec![{fixed}], {rest_text})",
            helper = smelt_stdlib::runtime_symbols::host::ARGUMENTS_OBJECT,
            fixed = fixed_texts.join(", "),
        ));
        self.rendered_at_type(&call, unknown_ty, dest_ty, &self.render_scope())
    }

    /// Converts a blocking HTTP GET operation to Rust text.
    pub(super) fn http_get_expr(&self, url: &Operand) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(url)?),
            Some(Type::String | Type::PyStr)
        ) {
            return Err(EmitError::new("HTTP GET URL must be a string"));
        }
        Ok(RustExpr::atom(format!(
            "reqwest::blocking::get({}).expect(\"HTTP GET failed\").text().expect(\"HTTP response body read failed\")",
            self.operand_expr(url)?.delimited()
        )))
    }

    /// Emit a read of a mutable global's thread-local cell.
    ///
    /// Copy primitives read through `Cell::get`; strings clone the borrowed
    /// `RefCell<String>` contents.
    pub(super) fn global_get_expr(&self, global: u32) -> Result<RustExpr, EmitError> {
        let name = crate::globals::global_static_name(self.mir, global);
        let ty = self.global_ty(global)?;
        if Self::global_uses_copy_cell(self.mir.types.get(ty)) {
            Ok(RustExpr::chain(format!("{name}.with(::std::cell::Cell::get)")))
        } else {
            Ok(RustExpr::chain(format!("{name}.with(|smelt_value| smelt_value.borrow().clone())")))
        }
    }

    /// Force a mutable global's lazy thread-local initializer without reading
    /// its value: `NAME.with(|_| ())`. Used for a slot read whose result is
    /// never consumed.
    pub(super) fn global_touch_expr(&self, global: u32) -> RustExpr {
        RustExpr::atom(crate::globals::global_static_name(self.mir, global)).postfix(".with(|_| ())")
    }

    /// Whether a mutable global of this type is backed by a `Cell` rather than
    /// a `RefCell`.
    ///
    /// Kept in sync with `emit_mutable_globals`, which decides the cell type:
    /// only the `Copy` primitives get a `Cell`, because a `Cell` read moves the
    /// value out and a non-`Copy` value cannot be read that way. Everything
    /// else — a `String`, a record, a list, a map — uses a `RefCell` and is read
    /// by cloning the borrow.
    fn global_uses_copy_cell(ty: Option<&Type>) -> bool {
        matches!(ty, Some(Type::Float | Type::Int | Type::Bool))
    }

    /// Emit a store into a mutable global's thread-local cell.
    ///
    /// The stored value is hoisted to a temporary so the block evaluates to the
    /// stored value, letting `++`/`+=` compose as expressions. Copy primitives
    /// use `Cell::set`; strings replace the `RefCell<String>` contents.
    ///
    /// The value is STORED at the global's declared type and the block's result
    /// is then produced at `dest_ty`, which are not always the same type. In
    /// JavaScript `x = v` evaluates to `v`, so the assignment can be consumed
    /// somewhere that erased it — a `Record<string, RegExp>` global assigned
    /// where the surrounding expression is typed `unknown` yielded the concrete
    /// record into a `SmeltUnknown` slot (2 errors in the hono slice). Coercing
    /// the trailing value separately keeps the store precise while letting the
    /// expression's own type decide what the block hands back; where the two
    /// agree the coercion renders nothing.
    pub(super) fn global_set_expr(
        &self,
        global: u32,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let name = crate::globals::global_static_name(self.mir, global);
        let ty = self.global_ty(global)?;
        let value_text = self.value_at_type(value, ty)?;
        let result_text = self.rendered_at_type(&RustExpr::path("smelt_global_value"), ty, dest_ty, &self.render_scope())?;
        if Self::global_uses_copy_cell(self.mir.types.get(ty)) {
            Ok(RustExpr::atom(format!(
                "{{ let smelt_global_value = {value_text}; {name}.with(|smelt_value| smelt_value.set(smelt_global_value)); {result_text} }}", value_text = value_text.delimited(), result_text = result_text.delimited()
            )))
        } else {
            Ok(RustExpr::atom(format!(
                "{{ let smelt_global_value = {value_text}; {name}.with(|smelt_value| *smelt_value.borrow_mut() = smelt_global_value.clone()); {result_text} }}", value_text = value_text.delimited(), result_text = result_text.delimited()
            )))
        }
    }

    /// Emit a write *through* a mutable global into its `thread_local!` cell.
    ///
    /// The whole statement is one `with` closure so the `RefCell` borrow lives
    /// exactly as long as the mutation and no copy of the contained value is
    /// ever made. That is the point of `Place::Global`: a `GlobalGet` would
    /// clone, which shares the store for a handle type (`SmeltRecord`) and
    /// deep-copies for a value type (`HashMap`), so a write applied to the
    /// clone is correct for one and silently lost for the other.
    ///
    /// # Both operands are evaluated before `borrow_mut()`
    ///
    /// If the index or the right-hand side themselves read the same global
    /// (`cache[cache_key()] = v`), evaluating them inside the borrow is a
    /// `RefCell` double-borrow **panic at runtime**, not a compile error. So
    /// they are hoisted to `let` bindings above the borrow. This is the one
    /// detail that cannot be left to the reader.
    ///
    /// A `Cell` global never reaches here: a `Cell` holds only `Copy`
    /// primitives, and a primitive can be neither indexed nor have a field
    /// written, so the path is `RefCell`-only by construction. That is checked
    /// rather than assumed.
    pub(super) fn global_place_assign_expr(
        &self,
        global: u32,
        projection: &smelt_mir::GlobalProjection,
        value: &Rvalue,
    ) -> Result<RustExpr, EmitError> {
        use smelt_mir::GlobalProjection;

        let name = crate::globals::global_static_name(self.mir, global);
        let ty = self.global_ty(global)?;
        if Self::global_uses_copy_cell(self.mir.types.get(ty)) {
            return Err(EmitError::new(
                "internal: a write through a mutable global reached a Copy-primitive cell,                  which has no field or index to write",
            ));
        }
        let Some(Type::Dict(key_ty, item_ty)) = self.mir.types.get(ty).cloned() else {
            // Only the map-like shapes are lowered. Anything else (a list, a
            // class instance, an erased value) would need its own write
            // spelling, and guessing one is how a wrong value gets emitted with
            // no diagnostic. The frontend keeps a blocker for these.
            return Err(EmitError::new(format!(
                "write through a mutable global is only lowered for Record/Map globals                  (global {global} is {:?})",
                self.mir.types.get(ty)
            )));
        };
        let rendered_value = self.rvalue_expr_for_dest(value, item_ty)?;
        let key_text = match projection {
            GlobalProjection::Field(field) => self.dict_field_key_expr(key_ty, *field)?,
            GlobalProjection::Index { index, .. } => {
                if self.mir.types.get(key_ty).is_some_and(Type::is_text) {
                    self.string_like_operand_expr(index, "global index")?
                } else {
                    self.value_at_type(index, key_ty)?
                }
            }
        };
        Ok(RustExpr::chain(format!(
            "{name}.with(|smelt_global_cell| {{              let smelt_global_key = {key_text};              let smelt_global_value = {rendered_value};              smelt_global_cell.borrow_mut().insert(smelt_global_key, smelt_global_value); }})", key_text = key_text.delimited(), rendered_value = rendered_value.delimited()
        )))
    }

    /// Look up the primitive type of a mutable global by index.
    pub(super) fn global_ty(&self, global: u32) -> Result<TypeId, EmitError> {
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
    pub(super) fn external_class_instance_expr(
        &self,
        class: Symbol,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let class_name = self.symbol_name(class)?;
        if crate::stdlib::stdlib_class_of_class_symbol(self.mir, class)
            == Some(smelt_stdlib::StdlibClass::RegExp)
        {
            let pattern = args.first().map_or_else(
                || Ok(RustExpr::string_literal("")),
                |arg| self.string_like_operand_expr(arg, "RegExp pattern"),
            )?;
            let flags = args.get(1).map_or_else(
                || Ok(RustExpr::string_literal("")),
                |arg| self.string_like_operand_expr(arg, "RegExp flags"),
            )?;
            return Ok(RustExpr::atom(format!("SmeltRegExp::new({pattern}, {flags})", pattern = pattern.delimited(), flags = flags.delimited())));
        }
        let args_text = args
            .iter()
            .map(|arg| self.operand_expr(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let args_tuple_text = if args_text.is_empty() {
            "()".to_owned()
        } else {
            format!("({args_text},)")
        };
        Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "{{ let _smelt_external_args = {args_tuple_text}; let _smelt_external = Vec::from([(\"__class\".to_owned(), SmeltUnknown::String({class_name:?}.into()))]); SmeltUnknown::Object(SmeltObject::new(_smelt_external)) }}"
        ))))
    }
}
