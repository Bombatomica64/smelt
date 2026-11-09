//! Emission for JavaScript `[[Construct]]` and value-target `instanceof`.
//!
//! Both nodes ask a question about a function value that only the runtime can
//! answer — what the callee's `prototype` object is, and what a value's
//! prototype chain reaches — so both lower to one prelude helper each
//! (`smelt_construct`, `smelt_instance_of_value`; see
//! `smelt-runtime`'s `function_object.rs`). The emitter's whole job here is to erase
//! the operands to `SmeltUnknown`, which is the honest ABI: the constructor is
//! reached through a value whose identity is only known at runtime, and the
//! constructed object's shape is whatever the constructor decided.

use crate::rust::RustExpr;
use smelt_hir::TypeId;
use smelt_mir::Operand;

use super::{EmitError, FunctionEmitter};

impl FunctionEmitter<'_> {
    /// Render JavaScript `new callee(args)` through a function value.
    ///
    /// The callee is erased because construction reaches it as a VALUE: a
    /// typed `Rc<dyn Fn(..)>` and a `SmeltUnknown::Function` are the same
    /// JavaScript function, and the runtime helper needs the erased form to
    /// read its `prototype` property and to install a receiver for the call.
    /// The construction itself yields `SmeltUnknown` — a constructor decides
    /// its own result — so the value is extracted at the destination type,
    /// which is what lets `new f()` sit in a statement position (`()`) or feed
    /// a concretely typed binding.
    pub(super) fn construct_expr(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let callee_text = self.erase(callee)?;
        let arg_texts = args
            .iter()
            .map(|arg| self.erase(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let call_text = RustExpr::chain(format!("smelt_construct({callee_text}, ::std::vec![{arg_texts}])", callee_text = callee_text.delimited()));
        self.extract_rendered(&call_text, dest_ty, &self.render_scope())
    }

    /// Render `new value.constructor(args)` on an erased `value`.
    pub(super) fn construct_like_expr(
        &self,
        value: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let value_text = self.erase(value)?;
        let arg_texts = args
            .iter()
            .map(|arg| self.erase(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let call_text = RustExpr::chain(format!("smelt_construct_like({value_text}, ::std::vec![{arg_texts}])", value_text = value_text.delimited()));
        self.extract_rendered(&call_text, dest_ty, &self.render_scope())
    }

    /// Render a class read as a value.
    ///
    /// Which classes it covers is computed here, where every class is known: an
    /// instance only records its own class, so `instanceof` needs the subclass set.
    pub(super) fn class_value(
        &self,
        class: smelt_hir::Symbol,
        constructor: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let mut covered = vec![self.symbol_source_name(class)?];
        for candidate in &self.mir.classes {
            let mut base = candidate.base;
            while let Some(name) = base {
                if name == class {
                    covered.push(self.symbol_source_name(candidate.name)?);
                    break;
                }
                base = crate::classes::find_class(self.mir, name).and_then(|found| found.base);
            }
        }
        let constructor_text =
            constructor.map_or_else(|| Ok(RustExpr::undefined()), |ctor| self.erase(ctor))?;
        // Static methods are own properties of the class object
        // (`class_proto::class_static_entries`).
        let mut statics = crate::class_proto::class_static_entries(self.mir, self.context, class)?
            .into_iter()
            .map(|(key, adapter)| format!("({key:?}.to_owned(), {adapter})", adapter = adapter.delimited()))
            .collect::<Vec<_>>();
        statics.extend(self.class_value_static_hooks(class)?);
        let statics_text = if statics.is_empty() {
            "Vec::new()".to_owned()
        } else {
            format!("vec![{}]", statics.join(", "))
        };
        Ok(RustExpr::chain(format!(
            "smelt_class_value({:?}, {:?}, {constructor_text}, {statics_text})",
            covered[0],
            covered.join(","), constructor_text = constructor_text.delimited()
        )))
    }

    /// The live-static hooks of a class value: a `__smelt_static_get` closure
    /// reading every mutable static the class declares or inherits from its
    /// storage global, and a `__smelt_static_set` closure writing the class's
    /// OWN mutable statics (answering `true` for a key it stored). Empty for
    /// a class with no mutable static. The runtime asks them on a read of a
    /// missing own property and on every property write
    /// (`smelt_class_static_write`), so a class value never holds a copy that
    /// a direct `Box.PI = v` would leave stale.
    fn class_value_static_hooks(&self, class: smelt_hir::Symbol) -> Result<Vec<String>, EmitError> {
        let mut readable = Vec::new();
        let mut writable = Vec::new();
        let mut current = crate::classes::find_class(self.mir, class);
        let mut own = true;
        let mut seen = std::collections::HashSet::new();
        while let Some(found) = current {
            for field in &found.static_fields {
                let Some(global) = field.global else {
                    continue;
                };
                let key = self.symbol_source_name(field.name)?.to_owned();
                if !seen.insert(key.clone()) {
                    continue;
                }
                readable.push((key.clone(), global, field.ty));
                if own {
                    writable.push((key, global, field.ty));
                }
            }
            own = false;
            current = found.base.and_then(|base| crate::classes::find_class(self.mir, base));
        }
        if readable.is_empty() {
            return Ok(Vec::new());
        }
        let key_text = "let smelt_static_key = match smelt_args.first() { Some(SmeltUnknown::String(smelt_key)) => smelt_key.to_string(), _ => return Ok(SmeltUnknown::Undefined) };";
        let mut get_arms = String::new();
        for (key, global, ty) in &readable {
            let read = self.erase_rendered(&self.global_get_expr(*global)?, *ty)?;
            get_arms.push_str(&format!("{key:?} => {}, ", read.delimited()));
        }
        let mut set_arms = String::new();
        for (key, global, ty) in &writable {
            let value = self.extract_rendered(&RustExpr::path("smelt_static_value"), *ty, &self.render_scope())?;
            let store = self.global_store_text(*global, &value)?;
            set_arms.push_str(&format!("{key:?} => {{ {store}; true }}, "));
        }
        Ok(vec![
            self.carrier_format(self.carrier()?, format_args!(
                "(\"__smelt_method:{get_hook}\".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {{ {key_text} Ok(match smelt_static_key.as_str() {{ {get_arms}_ => SmeltUnknown::Undefined }}) }})))",
                get_hook = smelt_stdlib::runtime_symbols::markers::SMELT_STATIC_GET
            )),
            self.carrier_format(self.carrier()?, format_args!(
                "(\"__smelt_method:{set_hook}\".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {{ {key_text} let smelt_static_value = smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Undefined); let _ = &smelt_static_value; Ok(SmeltUnknown::Bool(match smelt_static_key.as_str() {{ {set_arms}_ => false }})) }})))",
                set_hook = smelt_stdlib::runtime_symbols::markers::SMELT_STATIC_SET
            )),
        ])
    }

    /// Render JavaScript `value instanceof target` for a runtime constructor.
    ///
    /// Unlike [`FunctionEmitter::instance_of_expr`], whose target is a class
    /// name known at compile time, the target here is a value: the answer is a
    /// prototype-chain walk, so both operands are erased and handed to the
    /// runtime predicate.
    pub(super) fn instance_of_value(
        &self,
        value: &Operand,
        target: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let value_text = self.erase(value)?;
        let target_text = self.erase(target)?;
        Ok(RustExpr::chain(format!(
            "smelt_instance_of_value(&{value_text}, &{target_text})", value_text = value_text.tight(), target_text = target_text.tight()
        )))
    }
}
