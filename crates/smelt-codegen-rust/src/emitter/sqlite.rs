//! Rust emission for `node:sqlite`'s `DatabaseSync` and `StatementSync`.
//!
//! Every operation is a method call on the concrete structs emitted by
//! `crate::sqlite_prelude`, and every one can fail at run time, so each call
//! ends in `?` (the MIR throwing pass marks them throwing). The values that
//! stay typed are typed: SQL text is a `&str`, a prepared statement is a
//! `SmeltSqliteStatement`, and `run()`'s `{ changes, lastInsertRowid }` is a
//! number record. Only the two genuinely dynamic boundaries are erased:
//! statement PARAMETERS, which `SQLite` binds by position or name from whatever
//! values the call supplies, and ROWS, whose columns the SQL text decides at
//! run time — the same boundary as a parsed JSON document.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit `new DatabaseSync(path)`.
    pub(super) fn sqlite_open_text(&self, path: &Operand) -> Result<String, EmitError> {
        let string_ty = self.type_id(Type::String)?;
        let path_text = self.value_at_type(path, string_ty)?;
        Ok(format!("SmeltSqliteDatabase::open(&{path_text})?"))
    }

    /// Emit one database or statement operation.
    ///
    /// Database operations take the SQL text as their only argument; statement
    /// operations take any number of parameters, each erased at the binding
    /// boundary. Results are rendered at their own Rust type and then
    /// converted to the destination's through the ordinary coercion.
    pub(super) fn sqlite_op_text(
        &self,
        op: smelt_hir::SqliteOp,
        receiver: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        use smelt_hir::SqliteOp;
        let receiver_text = self.operand_text(receiver)?;
        match op {
            SqliteOp::Exec | SqliteOp::Prepare => {
                let Some(sql) = args.first() else {
                    return Err(EmitError::new("`DatabaseSync` exec/prepare requires SQL text"));
                };
                let string_ty = self.type_id(Type::String)?;
                let sql_text = self.value_at_type(sql, string_ty)?;
                let method = if op == SqliteOp::Exec { "exec" } else { "prepare" };
                Ok(format!("{receiver_text}.{method}(&{sql_text})?"))
            }
            SqliteOp::Close => Ok(format!("{receiver_text}.close()?")),
            SqliteOp::All | SqliteOp::Get | SqliteOp::Run => {
                let unknown_ty = self.type_id(Type::Unknown)?;
                let parameters = args
                    .iter()
                    .map(|arg| self.sqlite_parameter_text(arg))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                let scope = self.render_scope();
                match op {
                    SqliteOp::All => {
                        let rows_ty = self.type_id(Type::List(unknown_ty))?;
                        let text = format!("{receiver_text}.smelt_sqlite_all(&[{parameters}])?");
                        self.value_at_type_text(&text, rows_ty, dest_ty, &scope)
                    }
                    SqliteOp::Get => {
                        let text = format!("{receiver_text}.smelt_sqlite_get(&[{parameters}])?");
                        self.value_at_type_text(&text, unknown_ty, dest_ty, &scope)
                    }
                    _ => {
                        let float_ty = self.type_id(Type::Float)?;
                        let string_ty = self.type_id(Type::String)?;
                        let changes_ty = self.type_id(Type::Dict(string_ty, float_ty))?;
                        let text = format!(
                            "{{ let (smelt_changes, smelt_last_insert_rowid) = {receiver_text}.run(&[{parameters}])?; SmeltRecord::from([(\"changes\".to_owned(), smelt_changes), (\"lastInsertRowid\".to_owned(), smelt_last_insert_rowid)]) }}"
                        );
                        self.value_at_type_text(&text, changes_ty, dest_ty, &scope)
                    }
                }
            }
        }
    }

    /// Render one statement parameter as a `&dyn SmeltSqliteBind` element.
    ///
    /// A parameter whose type SQLite can store directly — a number, a string, a
    /// boolean (which Node refuses at run time, as the runtime does), an
    /// optional of one, or a string-keyed record of them for named parameters —
    /// binds at its own Rust type, the way `rusqlite::params![..]` would. Any
    /// other value is erased first and binds by its run-time tag: that is the
    /// case Node itself decides at run time (an `unknown`, a class instance
    /// used as a named-parameter object).
    fn sqlite_parameter_text(&self, arg: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(arg)?;
        let target = if self.binds_at_own_type(ty) {
            ty
        } else {
            self.type_id(Type::Unknown)?
        };
        Ok(format!("&{}", self.value_at_type(arg, target)?))
    }

    /// Whether a value of `ty` has a `SmeltSqliteBind` impl at its own type.
    fn binds_at_own_type(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Float | Type::Int | Type::String | Type::Bool | Type::Unknown) => true,
            Some(Type::Optional(inner)) => {
                matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Float | Type::Int | Type::String | Type::Bool)
                )
            }
            Some(Type::Dict(key, value)) => {
                matches!(self.mir.types.get(*key), Some(Type::String))
                    && self.dict_uses_smelt_record(*key)
                    && matches!(
                        self.mir.types.get(*value),
                        Some(Type::Float | Type::Int | Type::String | Type::Bool | Type::Unknown)
                    )
            }
            _ => false,
        }
    }
}
