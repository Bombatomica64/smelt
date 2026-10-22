//! TypeScript lowering for `node:sqlite`'s synchronous database.
//!
//! Two entry points:
//!
//! * [`ModuleBuilder::sqlite_database_constructor_expression`] for
//!   `new DatabaseSync(path)`, recognized by where the constructor was
//!   IMPORTED from (`node:sqlite`), not by its spelling;
//! * [`ModuleBuilder::dispatch_sqlite_method`] in the builtin call-handler
//!   chain, for the database's `exec`/`prepare`/`close` and the statement's
//!   `all`/`get`/`run`, admitted by the receiver's modeled class.
//!
//! Result types are as precise as the operation allows: `prepare` answers a
//! `StatementSync`, `run` a `Record<string, number>` holding `changes` and
//! `lastInsertRowid`, and `exec`/`close` nothing. Rows are `unknown` (a list
//! of them for `all`), because the SQL text — not the program's types —
//! decides their columns at run time; a source `as TodoRow[]` recovers the
//! row shape through the ordinary checked cast, as it would for parsed JSON.
//! Node's own declaration types them as `Record<string, SQLOutputValue>`, so
//! this is the same dynamic boundary, not an erasure of a known shape.

use super::super::ModuleBuilder;
use crate::error::SmeltError;
use oxc::ast::ast::Expression;
use smelt_hir::{Body, Expr, ExprKind, SqliteOp, Type};

impl ModuleBuilder<'_> {
    /// Lower `new DatabaseSync(path)`, or answer `None` for any other `new`.
    ///
    /// Node also accepts a `URL` or a `Buffer` path and an options object
    /// (`open`, `readOnly`, `timeout`, ...); none of those is modeled, so each
    /// is a named blocker rather than an argument that is accepted and ignored.
    pub(in crate::lowering) fn sqlite_database_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.host_module_export_path(&new_expr.callee)
            != Some((smelt_stdlib::HostModuleId::Sqlite, "DatabaseSync".to_owned()))
        {
            return Ok(None);
        }
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let [argument] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                span,
                "`new DatabaseSync` is modeled with exactly one argument, the database path",
            ));
        };
        let path = self.argument(argument, body)?;
        if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, path)), Some(Type::String)) {
            return Err(SmeltError::unsupported(
                span,
                "`new DatabaseSync` is modeled for a string path only",
            ));
        }
        let ty = self.sqlite_class_type("DatabaseSync");
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SqliteOpen { path },
            ty,
            span,
        })))
    }

    /// Dispatch a modeled `DatabaseSync` or `StatementSync` method.
    pub(in crate::lowering) fn dispatch_sqlite_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let database_rule = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::SqliteDatabase,
            member_name,
        );
        let statement_rule = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::SqliteStatement,
            member_name,
        );
        if database_rule.is_none() && statement_rule.is_none() {
            return Ok(None);
        }
        if self.user_class_shadows("DatabaseSync") || self.user_class_shadows("StatementSync") {
            return Ok(None);
        }
        // `exec`, `get`, `all` and `run` are also `RegExp`'s and `Map`'s
        // method names, so most receivers lowered here are not a database:
        // the probe is rolled back on that path (as the callback-closure probe
        // does) and the dispatcher that owns the call lowers the receiver itself.
        let exprs_before = body.exprs.len();
        let bodies_before = self.ctx.krate.bodies.len();
        let items_before = self.ctx.krate.items.len();
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_class = self.stdlib_class_of_type(Self::expr_ty(body, receiver));
        let span = self.span(call.span.start, call.span.end);
        let (op, sql_arguments) = match (receiver_class, member_name) {
            (Some(smelt_stdlib::StdlibClass::SqliteDatabase), "exec") => (SqliteOp::Exec, Some(1)),
            (Some(smelt_stdlib::StdlibClass::SqliteDatabase), "prepare") => {
                (SqliteOp::Prepare, Some(1))
            }
            (Some(smelt_stdlib::StdlibClass::SqliteDatabase), "close") => (SqliteOp::Close, Some(0)),
            (Some(smelt_stdlib::StdlibClass::SqliteStatement), "all") => (SqliteOp::All, None),
            (Some(smelt_stdlib::StdlibClass::SqliteStatement), "get") => (SqliteOp::Get, None),
            (Some(smelt_stdlib::StdlibClass::SqliteStatement), "run") => (SqliteOp::Run, None),
            _ => {
                body.exprs.truncate(exprs_before);
                if self.ctx.krate.items.len() == items_before {
                    self.ctx.krate.bodies.truncate(bodies_before);
                }
                return Ok(None);
            }
        };
        if let Some(expected) = sql_arguments
            && call.arguments.len() != expected
        {
            return Err(SmeltError::unsupported(
                span,
                format!("`DatabaseSync.{member_name}` is modeled with {expected} argument(s)"),
            ));
        }
        if call
            .arguments
            .iter()
            .any(oxc::ast::ast::Argument::is_spread)
        {
            return Err(SmeltError::unsupported(
                span,
                format!("spread arguments to `{member_name}` are not modeled yet"),
            ));
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.sqlite_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SqliteOp {
                op,
                receiver,
                args,
            },
            ty,
            span,
        })))
    }

    /// The HIR type a database or statement operation answers.
    fn sqlite_op_result_type(&mut self, op: SqliteOp) -> smelt_hir::TypeId {
        match op {
            SqliteOp::Exec | SqliteOp::Close => self.ctx.krate.types.intern(Type::None),
            SqliteOp::Prepare => self.sqlite_class_type("StatementSync"),
            SqliteOp::All => {
                let row_ty = self.ctx.krate.types.intern(Type::Unknown);
                self.ctx.krate.types.intern(Type::List(row_ty))
            }
            SqliteOp::Get => self.ctx.krate.types.intern(Type::Unknown),
            SqliteOp::Run => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                self.ctx.krate.types.intern(Type::Dict(string_ty, float_ty))
            }
        }
    }

    /// Intern one of the two modeled `node:sqlite` class types.
    fn sqlite_class_type(&mut self, name: &str) -> smelt_hir::TypeId {
        let name = self.intern_type_name(name);
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }
}
