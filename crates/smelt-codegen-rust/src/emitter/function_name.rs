//! Emission of JavaScript `Function.prototype.name` reads ([`Rvalue::FunctionName`]).
//!
//! A function's name is fixed when the function value is created: a
//! declaration's own name, or the binding or property name the spec's
//! `NamedEvaluation` hands an anonymous function (`const f = () => 1` is `"f"`).
//! The frontend records it on the closure (`MirClosure::name`), so a read whose
//! operand provably holds one closure definition is a compile-time string, the
//! constant a hand port would write.
//!
//! A read whose operand's closure is not statically known (a function
//! parameter, a list element, a local reassigned to different functions) has no
//! name storage in a typed `Rc<dyn Fn>` or `&dyn Fn` handle, so it answers the
//! empty string, the name of an anonymous function.

use super::*;
use crate::rust::RustExpr;

/// How many `a = b` copies are followed back to a closure definition.
const COPY_CHAIN_LIMIT: usize = 16;

impl FunctionEmitter<'_> {
    /// Emit `operand.name` for a function operand, typed at `dest_ty`.
    pub(super) fn function_name_expr(
        &self,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let name = self.static_function_name(operand)?.unwrap_or_default();
        Ok(match self.mir.types.get(dest_ty) {
            Some(Type::String) => RustExpr::js_string_literal(&name),
            _ => RustExpr::string_literal(&name),
        })
    }

    /// The name of the one closure `operand` can hold, when that is known.
    ///
    /// Only a local assigned exactly once is followed, through plain copies, to
    /// its `Rvalue::Closure` or a method read off a class instance: a local
    /// written twice may hold either value at the read, and a parameter is
    /// written by the caller.
    fn static_function_name(&self, operand: &Operand) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(mut place) | Operand::Move(mut place)) = operand.clone() else {
            return Ok(None);
        };
        for _ in 0..COPY_CHAIN_LIMIT {
            if let Place::Field { base, field } = place {
                return self.method_value_name(base, field);
            }
            let Place::Local(local) = place else {
                return Ok(None);
            };
            match self.single_assigned_value(local) {
                Some(Rvalue::Closure { id, .. }) => {
                    let closure = self
                        .mir
                        .closures
                        .get(id_index(id.0, "closure index does not fit usize")?)
                        .ok_or_else(|| EmitError::new("function name read of a missing closure"))?;
                    return Ok(Some(
                        closure
                            .name
                            .and_then(|name| self.symbol_source_name(name).ok())
                            .unwrap_or_default()
                            .to_owned(),
                    ));
                }
                Some(Rvalue::Use(Operand::Copy(source) | Operand::Move(source))) => {
                    place = source.clone();
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    /// The name of a method read off a class instance (`k.m`): the method's
    /// property name, whichever class in the chain declares it. A data field
    /// holding a function reports the stored function's name, which is not
    /// known here.
    fn method_value_name(&self, base: LocalId, field: Symbol) -> Result<Option<String>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty) else {
            return Ok(None);
        };
        if self.class_has_named_field(base_ty, field)
            || self.find_class_method_function(*name, field).is_none()
        {
            return Ok(None);
        }
        Ok(Some(self.symbol_source_name(field)?.to_owned()))
    }

    /// The value of the one assignment writing a function local.
    ///
    /// `let f: F;` writes an `undefined` placeholder first; TypeScript's
    /// definite-assignment check rejects a read before the real assignment, so
    /// that write is never observed and does not count. Any other second write
    /// (`f = g` later) answers `None`, as does a write through a dict entry.
    fn single_assigned_value(&self, local: LocalId) -> Option<&Rvalue> {
        let writes: Vec<&Rvalue> = self
            .function
            .blocks
            .iter()
            .flat_map(|block| &block.statements)
            .filter_map(|statement| {
                if let Statement::Assign { dest, value } = statement
                    && *dest == local
                {
                    return Some(value);
                }
                if let Statement::AssignPlace {
                    place: Place::Local(dest),
                    value,
                } = statement
                    && *dest == local
                {
                    return Some(value);
                }
                None
            })
            .collect();
        // A dict entry bound to the local is a write the index counts and this
        // scan does not; it disqualifies the local.
        if writes.len() != self.local_assignment_count(local) {
            return None;
        }
        let mut observed = writes
            .into_iter()
            .filter(|value| !matches!(value, Rvalue::Use(Operand::Const(Constant::Undefined))));
        let value = observed.next()?;
        observed.next().is_none().then_some(value)
    }
}

impl FunctionEmitter<'_> {
    /// A function value used at an object type (`const n: { name: string;
    /// length: number } = hello`): the record built from the properties every
    /// function owns.
    ///
    /// TypeScript accepts a function at an object type whose members are all
    /// `Function.prototype` members or optional, so the record's `name` and
    /// `length` are read off the function and every other field takes its
    /// `undefined` (an optional member the function lacks). With the `operand`
    /// at hand the reads are the ones `f.name` and `f.length` emit (a static
    /// name, the recorded arity); without it (a value already rendered to
    /// text) the name is the anonymous `""` and the arity is the signature's.
    ///
    /// Answers `None` when `target` is not a record type or has a required
    /// member other than `name` and `length`, which keeps the caller's
    /// conversion.
    pub(super) fn function_record_expr(
        &self,
        operand: Option<&Operand>,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(Type::Function(function)) = self.mir.types.get(source) else {
            return Ok(None);
        };
        let Some(Type::Class { name: class, args }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        if self.is_erased_class_type(target)
            || (!self.is_interface_record_type(target) && !self.mir.has_class(*class))
        {
            return Ok(None);
        }
        let Some(fields) = self.structural_record_fields(target) else {
            return Ok(None);
        };
        let signature_length = function
            .required_params
            .unwrap_or_else(|| function.rest.unwrap_or(function.params.len()));
        let mut parts = Vec::new();
        for field in fields {
            let field_ty = self.mir.types.get(field.ty);
            let read = match (self.symbol_source_name(field.name)?, field_ty) {
                ("name", Some(Type::String)) => Some(match operand {
                    Some(function_value) => self.function_name_expr(function_value, field.ty)?,
                    None => RustExpr::js_string_literal(""),
                }),
                ("length", Some(Type::Float | Type::Int)) => Some(match operand {
                    Some(function_value) => self.len_expr(function_value, field.ty)?,
                    None if matches!(field_ty, Some(Type::Float)) => {
                        RustExpr::atom(format!("{signature_length}.0"))
                    }
                    None => RustExpr::atom(format!("{signature_length}")),
                }),
                _ => None,
            };
            let accessor = crate::accessor_prelude::is_accessor_field(self.mir, *class, field.name);
            let value = match read {
                Some(own) if accessor => crate::accessor_prelude::constant_expr(&own),
                Some(own) => {
                    self.record_field_value_at_slot_abi(own.into_string(), target, &field)?
                }
                // An optional member the function does not own reads `undefined`.
                None if matches!(field_ty, Some(Type::Optional(..))) => {
                    let default = self.default_value(field.ty)?;
                    if accessor {
                        crate::accessor_prelude::constant_expr(&default)
                    } else {
                        default
                    }
                }
                // A required member this conversion cannot read (a
                // `Function.prototype` method): keep the caller's conversion.
                None => return Ok(None),
            };
            let field_name = self.context.names().field_text(self.mir, field.name);
            parts.push(format!("{field_name}: {value}", value = value.delimited()));
        }
        Ok(Some(self.finish_record_literal(
            *class,
            args.len(),
            target,
            parts,
        )?))
    }
}
