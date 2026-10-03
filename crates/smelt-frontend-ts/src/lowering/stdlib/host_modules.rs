//! Lowering for the modeled Node host-module functions: `node:path` and the
//! `node:crypto` hasher (`createHash` / `Hash`).
//!
//! # Resolved from the import, not from the spelling
//!
//! A host-module function is recognized by WHERE ITS NAME CAME FROM: the
//! callee's root identifier must be an import whose specifier the registry
//! (`smelt_stdlib::host_modules`) models, and the export it names — after the
//! member path is appended — must be one this module implements. So every
//! import shape reaches the same rule:
//!
//! ```ts
//! import { join } from 'node:path'           // join(a, b)
//! import { join as posixJoin } from 'node:path/posix'
//! import path from 'node:path'               // path.join(a, b)
//! import * as path from 'path'               // path.posix.join(a, b)
//! import { createHash } from 'crypto'        // createHash('sha256')
//! ```
//!
//! and a program that binds its own `join` (or imports one from a source
//! module) is untouched: its name does not resolve to a host import at all.
//! That is also why these functions are not entries in the free-name
//! recognition table — `join` and `basename` are ordinary identifiers, and a
//! name-keyed rule would be exactly the special case `CLAUDE.md` forbids.
//!
//! # `path.posix`
//!
//! The profile is POSIX, where Node's `path` IS `path.posix` (and
//! `path.posix.posix` is too), so a `posix` segment anywhere in the member path
//! names the same module and is dropped before the export is looked up.
//!
//! # The hasher
//!
//! `createHash(algorithm)` answers the concrete `Hash` class (the generated
//! `SmeltHash`); `hash.update(data, enc?)` answers the same hash, so chains
//! lower as nested calls on one handle; `hash.digest(enc)` answers a `String`
//! and `hash.digest()` the digest bytes wrapped into the `Buffer` the source is
//! typed with. All three can throw Node's own errors, so they reach MIR as call
//! terminators (`HostModuleOp::is_fallible`).

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, CallExpression, Expression, StaticMemberExpression};
use smelt_hir::{Body, Expr, ExprKind, HashOp, HostModuleOp, Literal, PathOp, Type};
use smelt_stdlib::HostModuleId;

impl ModuleBuilder<'_> {
    /// Resolve an expression that names a host-module export to the module and
    /// the export's dotted member path.
    ///
    /// The root identifier must be an import from a modeled host module that
    /// the module does not shadow with its own binding. A default or namespace
    /// import is the module itself (empty path); a named import starts the
    /// path at its EXPORTED name, so an alias (`join as posixJoin`) resolves to
    /// `join`. For `node:path`, `posix` segments are dropped (see the module
    /// docs). Anything else — a call, an index, a local — answers `None`.
    pub(in crate::lowering) fn host_module_export_path(
        &self,
        expression: &Expression<'_>,
    ) -> Option<(HostModuleId, String)> {
        match expression {
            Expression::Identifier(identifier) => {
                self.host_module_identifier_export_path(identifier.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                self.host_module_member_export_path(member)
            }
            _ => None,
        }
    }

    /// [`Self::host_module_export_path`] for a bare identifier.
    pub(in crate::lowering) fn host_module_identifier_export_path(
        &self,
        name: &str,
    ) -> Option<(HostModuleId, String)> {
        // Only a LOCAL binding can shadow an import (a second top-level
        // declaration of the name is a TypeScript error); the crate-wide item
        // tables are no evidence either way, since another module may declare
        // an item of the same name (Hono's own `utils/crypto.ts` exports a
        // `createHash`).
        if self.scope.is_bound(name) {
            return None;
        }
        let id = smelt_stdlib::host_module_id(self.imports.import_source(name)?)?;
        let imported = self.imports.imported_name(name)?;
        let path = if matches!(imported, "default" | "*") {
            String::new()
        } else {
            imported.to_owned()
        };
        Some(Self::host_module_normalized_path(id, path))
    }

    /// [`Self::host_module_export_path`] for a static member read (`path.sep`,
    /// `path.posix.join`): the object's path with the property appended.
    pub(in crate::lowering) fn host_module_member_export_path(
        &self,
        member: &StaticMemberExpression<'_>,
    ) -> Option<(HostModuleId, String)> {
        let (id, base) = self.host_module_export_path(&member.object)?;
        let property = member.property.name.as_str();
        let path = if base.is_empty() {
            property.to_owned()
        } else {
            format!("{base}.{property}")
        };
        Some(Self::host_module_normalized_path(id, path))
    }

    /// Drop the `posix` segments of a `node:path` member path (see the module
    /// docs); every other module's path is its own.
    fn host_module_normalized_path(id: HostModuleId, path: String) -> (HostModuleId, String) {
        let path = if id == HostModuleId::Path {
            path.split('.')
                .filter(|segment| *segment != "posix")
                .collect::<Vec<_>>()
                .join(".")
        } else {
            path
        };
        (id, path)
    }

    /// Lower a call to a modeled host-module function.
    ///
    /// Registered in the builtin call-handler chain. Declines (`Ok(None)`) for
    /// a callee that does not resolve to a host import, and for an export this
    /// module does not implement — which then reaches the registry's declared
    /// blocker (`path.parse`) or the erased import path as before.
    pub(in crate::lowering) fn host_module_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((id, path)) = self.host_module_export_path(&call.callee) else {
            return Ok(None);
        };
        let op = match (id, path.as_str()) {
            (HostModuleId::Path, "join") => PathOp::Join,
            (HostModuleId::Path, "resolve") => PathOp::Resolve,
            (HostModuleId::Path, "normalize") => PathOp::Normalize,
            (HostModuleId::Path, "dirname") => PathOp::Dirname,
            (HostModuleId::Path, "basename") => PathOp::Basename,
            (HostModuleId::Path, "extname") => PathOp::Extname,
            (HostModuleId::Path, "relative") => PathOp::Relative,
            (HostModuleId::Path, "isAbsolute") => PathOp::IsAbsolute,
            (HostModuleId::Crypto, "createHash") => {
                return self.create_hash_call(call, body).map(Some);
            }
            _ => return Ok(None),
        };
        self.path_call(op, call, body).map(Some)
    }

    /// Lower one `node:path` function call.
    ///
    /// `join`/`resolve` pack their rest arguments (spreads included) into one
    /// `string[]`; the fixed-arity functions take their arguments as strings.
    /// Node's arity is enforced where it is observable: a missing required
    /// argument is a `TypeError` in Node, and refusing it here is the honest
    /// answer to a call TypeScript itself rejects.
    fn path_call(
        &mut self,
        op: PathOp,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let args = match op {
            PathOp::Join | PathOp::Resolve => {
                let list_ty = self.ctx.krate.types.intern(Type::List(string_ty));
                // `path.join()` with no segment at all is `.`: an empty list.
                let packed = if call.arguments.is_empty() {
                    body.push_expr(Expr {
                        kind: ExprKind::ListLit(Vec::new()),
                        ty: list_ty,
                        span,
                    })
                } else {
                    self.packed_spread_call_arguments(string_ty, call, body)?
                };
                // A spread of a list whose element type is erased (`[...args]`
                // over an `unknown[]`) is narrowed to the `string[]` the
                // function reads by a checked cast, where the program's own
                // erased value meets the typed surface.
                let packed = if Self::expr_ty(body, packed) == list_ty {
                    packed
                } else {
                    body.push_expr(Expr {
                        kind: ExprKind::UnknownCast {
                            value: packed,
                            target: list_ty,
                        },
                        ty: list_ty,
                        span,
                    })
                };
                Vec::from([packed])
            }
            PathOp::Relative => self.host_string_arguments(op.name(), call, 2, 2, body)?,
            PathOp::Basename => self.host_string_arguments(op.name(), call, 1, 2, body)?,
            PathOp::Normalize | PathOp::Dirname | PathOp::Extname | PathOp::IsAbsolute => {
                self.host_string_arguments(op.name(), call, 1, 1, body)?
            }
        };
        let ty = if op == PathOp::IsAbsolute {
            self.ctx.krate.types.intern(Type::Bool)
        } else {
            string_ty
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::HostModuleCall {
                op: HostModuleOp::Path(op),
                args,
            },
            ty,
            span,
        }))
    }

    /// Lower `createHash(algorithm)` to a fresh `Hash`.
    ///
    /// The second `options` argument only configures the XOF output length of
    /// `shake*`, which no modeled algorithm is, so it is refused rather than
    /// ignored.
    fn create_hash_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        let args = self.host_string_arguments("createHash", call, 1, 1, body)?;
        let ty = self.node_hash_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::HostModuleCall {
                op: HostModuleOp::Hash(HashOp::Create),
                args,
            },
            ty,
            span,
        }))
    }

    /// Dispatch `update`/`digest` on a concrete `Hash` receiver.
    ///
    /// Registered in the builtin call-handler chain; keyed on the RECEIVER's
    /// lowered type (the modeled `Hash` class), never on the member name, since
    /// `update` and `digest` are ordinary user method names.
    pub(in crate::lowering) fn dispatch_node_hash_method(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if !matches!(member_name, "update" | "digest") || !self.could_be_node_hash(member) {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        if !self.is_node_hash_type(Self::expr_ty(body, receiver)) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        if member_name == "update" {
            let [data, rest @ ..] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    span,
                    "`hash.update(data, inputEncoding?)` needs the data to hash",
                ));
            };
            let data = self.host_plain_argument(data, span, body)?;
            let mut args = Vec::from([receiver, data]);
            match rest {
                [] => {}
                [encoding] => args.push(self.host_string_argument(encoding, span, body)?),
                _ => {
                    return Err(SmeltError::unsupported(
                        span,
                        "`hash.update` takes the data and at most an input encoding",
                    ));
                }
            }
            let ty = Self::expr_ty(body, receiver);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::HostModuleCall {
                    op: HostModuleOp::Hash(HashOp::Update),
                    args,
                },
                ty,
                span,
            })));
        }
        match call.arguments.as_slice() {
            [] => {
                // The raw digest is a `Buffer` in Node. The bytes come back as
                // the concrete view and enter the SAME `Buffer` construction
                // `Buffer.from(view)` uses, so the value answers the whole
                // `Buffer` surface (`toString('hex')`, `equals`, indexing).
                let bytes_ty = self.byte_array_type();
                let bytes = body.push_expr(Expr {
                    kind: ExprKind::HostModuleCall {
                        op: HostModuleOp::Hash(HashOp::DigestBytes),
                        args: Vec::from([receiver]),
                    },
                    ty: bytes_ty,
                    span,
                });
                Ok(Some(self.buffer_record_from_args(Vec::from([bytes]), span, body)))
            }
            [encoding] => {
                let encoding = self.host_string_argument(encoding, span, body)?;
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::HostModuleCall {
                        op: HostModuleOp::Hash(HashOp::DigestText),
                        args: Vec::from([receiver, encoding]),
                    },
                    ty,
                    span,
                })))
            }
            _ => Err(SmeltError::unsupported(
                span,
                "`hash.digest` takes at most an output encoding",
            )),
        }
    }

    /// Cheap syntactic pre-check before lowering a `.update`/`.digest` receiver.
    ///
    /// The receiver is lowered to learn its type, and lowering it for every
    /// `x.update(..)` in a program would build throwaway HIR for each; a hash
    /// receiver is a `createHash(..)` call, an `update` chain on one, or a
    /// name/member whose type is decided after lowering.
    fn could_be_node_hash(&self, member: &StaticMemberExpression<'_>) -> bool {
        match &member.object {
            Expression::CallExpression(inner) => {
                matches!(
                    self.host_module_export_path(&inner.callee),
                    Some((HostModuleId::Crypto, path)) if path == "createHash"
                ) || matches!(
                    &inner.callee,
                    Expression::StaticMemberExpression(inner_member)
                        if inner_member.property.name == "update"
                            && self.could_be_node_hash(inner_member)
                )
            }
            Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::PrivateFieldExpression(_)
            | Expression::ThisExpression(_) => true,
            _ => false,
        }
    }

    /// Read a `node:path` data export (`sep`, `delimiter`) as its constant.
    ///
    /// Called from both read shapes: a named import (`import { sep }`) through
    /// the identifier path and a member read (`path.sep`) through the static
    /// member path. On the POSIX profile they are `/` and `:`.
    pub(in crate::lowering) fn host_module_value_expression(
        &mut self,
        (id, path): (HostModuleId, String),
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let value = match (id, path.as_str()) {
            (HostModuleId::Path, "sep") => "/",
            (HostModuleId::Path, "delimiter") => ":",
            _ => return None,
        };
        let span = self.span(start, end);
        let ty = self.ctx.krate.types.intern(Type::String);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(value.to_owned())),
            ty,
            span,
        }))
    }

    /// Lower between `min` and `max` positional string arguments.
    fn host_string_arguments(
        &mut self,
        name: &str,
        call: &CallExpression<'_>,
        min: usize,
        max: usize,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        if call.arguments.len() < min || call.arguments.len() > max {
            return Err(SmeltError::unsupported(
                span,
                format!("`{name}` takes between {min} and {max} string arguments"),
            ));
        }
        call.arguments
            .iter()
            .map(|argument| self.host_string_argument(argument, span, body))
            .collect()
    }

    /// Lower one argument that the host function reads as a `string`.
    ///
    /// A `string` (or `string | undefined`, for an optional suffix) passes
    /// through. An erased value is converted by a checked cast: the host
    /// function's parameter IS a string, so the boundary is the cast, exactly
    /// where the program's own erased value meets the typed surface. Anything
    /// else is refused rather than stringified, because Node throws
    /// `ERR_INVALID_ARG_TYPE` for it.
    fn host_string_argument(
        &mut self,
        argument: &Argument<'_>,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.host_plain_argument(argument, span, body)?;
        let ty = Self::expr_ty(body, value);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        match self.ctx.krate.types.get(ty) {
            Some(Type::String) => Ok(value),
            Some(Type::Optional(inner)) if *inner == string_ty => Ok(value),
            Some(Type::Unknown | Type::TypeParam { .. }) => Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value,
                    target: string_ty,
                },
                ty: string_ty,
                span,
            })),
            _ => Err(SmeltError::unsupported(
                span,
                "this host-module argument must be a string",
            )),
        }
    }

    /// Lower one non-spread argument of a host-module call.
    fn host_plain_argument(
        &mut self,
        argument: &Argument<'_>,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if matches!(argument, Argument::SpreadElement(_)) {
            return Err(SmeltError::unsupported(
                span,
                "a spread argument is not supported for this host-module function",
            ));
        }
        self.argument(argument, body)
    }

    /// Return the modeled `node:crypto` `Hash` class type.
    pub(in crate::lowering) fn node_hash_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("Hash");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `Hash` class.
    pub(in crate::lowering) fn is_node_hash_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::NodeHash)
            && !self.user_class_shadows("Hash")
    }
}
