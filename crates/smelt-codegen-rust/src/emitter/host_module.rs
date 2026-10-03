//! Rust emission for the modeled Node host-module calls.
//!
//! Every call lands on a helper `crate::host_module_prelude` emits, with the
//! operands rendered at the helper's own Rust types: a path function takes
//! `&str`/`&[String]` and answers a `String` or `bool`; a hasher member takes
//! the `SmeltHash` handle and the bytes or text it is fed. The only erased
//! operand is an `update` argument whose static type carries no shape (an
//! `unknown`, or a `string | Buffer` union), which is decided at run time
//! through the view's boundary adapter exactly as `crypto.subtle.digest`
//! reads its erased input.

use super::*;
use crate::host_module_prelude as prelude;
use smelt_hir::{HashOp, HostModuleOp, PathOp};

impl FunctionEmitter<'_> {
    /// Emit an INFALLIBLE host-module call (a `node:path` function).
    ///
    /// The hasher members never reach here: they are fallible, so MIR lowers
    /// them to `BuiltinFn::HostModule` call terminators (see
    /// [`Self::host_module_call_text`]).
    pub(super) fn host_module_rvalue_text(
        &self,
        host_op: HostModuleOp,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let HostModuleOp::Path(op) = host_op else {
            return Err(EmitError::new(
                "internal: a fallible host-module call reached the rvalue path",
            ));
        };
        let arg = |index: usize| {
            args.get(index).ok_or_else(|| {
                EmitError::new(format!("`{}` is missing argument {index}", op.name()))
            })
        };
        let text = |index: usize| -> Result<String, EmitError> {
            self.string_like_operand_text(arg(index)?, op.name())
        };
        Ok(match op {
            PathOp::Join | PathOp::Resolve => {
                let helper = if op == PathOp::Join {
                    prelude::PATH_JOIN_FN
                } else {
                    prelude::PATH_RESOLVE_FN
                };
                let list = self.operand_text(arg(0)?)?;
                format!("{helper}(&{})", list_read_text(&list))
            }
            PathOp::Normalize => format!("{}(&{})", prelude::PATH_NORMALIZE_FN, text(0)?),
            PathOp::Dirname => format!("{}(&{})", prelude::PATH_DIRNAME_FN, text(0)?),
            PathOp::Extname => format!("{}(&{})", prelude::PATH_EXTNAME_FN, text(0)?),
            PathOp::IsAbsolute => format!("{}(&{})", prelude::PATH_IS_ABSOLUTE_FN, text(0)?),
            PathOp::Relative => format!(
                "{}(&{}, &{})",
                prelude::PATH_RELATIVE_FN,
                text(0)?,
                text(1)?
            ),
            PathOp::Basename => {
                // The suffix is optional twice over: absent from the call, or
                // present with an `Optional<String>` type (`basename(p, ext?)`
                // forwarding its own optional parameter).
                let suffix = match args.get(1) {
                    None => "None".to_owned(),
                    Some(suffix) => match self.mir.types.get(self.operand_ty(suffix)?) {
                        Some(Type::Optional(_)) => {
                            format!("({}).as_deref()", self.operand_text(suffix)?)
                        }
                        _ => format!("Some({}.as_str())", text(1)?),
                    },
                };
                format!("{}(&{}, {suffix})", prelude::PATH_BASENAME_FN, text(0)?)
            }
        })
    }

    /// Emit a FALLIBLE host-module call (a `node:crypto` hasher member).
    ///
    /// Every rendering ends in `?` over a `Result`, which is what marks the
    /// call fallible to `emit_throwing_call_terminator`: the caught error is
    /// bound and control jumps to the handler, as for `JSON.parse`.
    pub(super) fn host_module_call_text(
        &self,
        host_op: HostModuleOp,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let HostModuleOp::Hash(op) = host_op else {
            return Err(EmitError::new(
                "internal: an infallible host-module call reached the call path",
            ));
        };
        let arg = |index: usize| {
            args.get(index).ok_or_else(|| {
                EmitError::new(format!("`{}` is missing argument {index}", op.name()))
            })
        };
        match op {
            HashOp::Create => {
                let algorithm = self.string_like_operand_text(arg(0)?, op.name())?;
                Ok(format!("{}(&{algorithm})?", prelude::HASH_CREATE_FN))
            }
            HashOp::Update => {
                let hash = self.operand_text(arg(0)?)?;
                self.hash_update_text(&hash, arg(1)?, args.get(2))
            }
            HashOp::DigestText => {
                let hash = self.operand_text(arg(0)?)?;
                let encoding = self.string_like_operand_text(arg(1)?, op.name())?;
                Ok(format!(
                    "{}(&{hash}, &{encoding})?",
                    prelude::HASH_DIGEST_TEXT_FN
                ))
            }
            HashOp::DigestBytes => {
                let hash = self.operand_text(arg(0)?)?;
                Ok(format!(
                    "{}(&{hash}).map(SmeltUint8Array::from_bytes)?",
                    prelude::HASH_DIGEST_BYTES_FN
                ))
            }
        }
    }

    /// Emit `hash.update(data, inputEncoding?)` by the data's own type.
    ///
    /// A `String` goes through the input encoding (default `utf8`); a
    /// byte-backed value — a typed-array view, a `DataView`, an `ArrayBuffer`,
    /// or a concrete union whose every arm is one — feeds its bytes and ignores
    /// the encoding, as Node does. Anything else is the dynamic boundary: the
    /// erased value is a string or a byte-backed record at run time, and the
    /// view's `SmeltFromUnknown` adapter reads the bytes of any such record
    /// (a `Buffer` included).
    fn hash_update_text(
        &self,
        hash: &str,
        data: &Operand,
        encoding_operand: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let encoding = encoding_operand
            .map(|operand| self.string_like_operand_text(operand, "hash.update encoding"))
            .transpose()?
            .unwrap_or_else(|| "\"utf8\".to_owned()".to_owned());
        let data_ty = self.operand_ty(data)?;
        if matches!(self.mir.types.get(data_ty), Some(Type::String)) {
            let text = self.string_like_operand_text(data, "hash.update data")?;
            return Ok(format!(
                "{}(&{hash}, &{text}, &{encoding})?",
                prelude::HASH_UPDATE_TEXT_FN
            ));
        }
        if self.is_typed_array_view_class_type(data_ty)?
            || self.operand_is_array_buffer(data)?
            || self.is_data_view_class_type(data_ty)?
        {
            return Ok(format!(
                "{}(&{hash}, &({}).to_bytes())?",
                prelude::HASH_UPDATE_BYTES_FN,
                self.operand_text(data)?
            ));
        }
        if let Some(bytes) = self.byte_source_union_bytes_text(&self.operand_text(data)?, data_ty)?
        {
            return Ok(format!(
                "{}(&{hash}, &{bytes})?",
                prelude::HASH_UPDATE_BYTES_FN
            ));
        }
        let erased = self.erase(data)?;
        Ok(format!(
            "{{ let smelt_value = {erased}; match &smelt_value {{ SmeltUnknown::String(text) => {text_fn}(&{hash}, &text.to_string(), &{encoding}), _ => {bytes_fn}(&{hash}, &SmeltTypedArray::smelt_from_unknown(smelt_value.clone()).to_bytes()) }} }}?",
            text_fn = prelude::HASH_UPDATE_TEXT_FN,
            bytes_fn = prelude::HASH_UPDATE_BYTES_FN,
        ))
    }

    /// Whether a type is the concrete `DataView` class.
    fn is_data_view_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::DataView))
    }

    /// The static result type of a fallible host-module call.
    ///
    /// The builtin callee carries only its op, so the type is found in the
    /// table the frontend filled when it typed the node: the `Hash` class for
    /// `createHash`/`update`, a `String` for an encoded digest and the
    /// `Uint8Array` view for the raw one.
    pub(super) fn host_module_call_source_ty(
        &self,
        op: HostModuleOp,
    ) -> Result<TypeId, EmitError> {
        let found = match op {
            HostModuleOp::Path(PathOp::IsAbsolute) => return self.type_id(Type::Bool),
            HostModuleOp::Path(_) | HostModuleOp::Hash(HashOp::DigestText) => {
                return self.type_id(Type::String);
            }
            HostModuleOp::Hash(HashOp::Create | HashOp::Update) => {
                self.stdlib_class_ty(smelt_stdlib::StdlibClass::NodeHash)
            }
            HostModuleOp::Hash(HashOp::DigestBytes) => self.mir.types.all().iter().position(|ty| {
                matches!(ty, Type::Class { name, args } if args.is_empty()
                    && self.symbol_name(*name).is_ok_and(|text| text == "Uint8Array")
                    && self.stdlib_class_of_symbol(*name) == Ok(Some(smelt_stdlib::StdlibClass::TypedArray)))
            }).and_then(|index| u32::try_from(index).ok()).map(TypeId),
        };
        found.ok_or_else(|| {
            EmitError::new(format!(
                "`{}` needs its result class interned in the type table",
                op.name()
            ))
        })
    }
}
