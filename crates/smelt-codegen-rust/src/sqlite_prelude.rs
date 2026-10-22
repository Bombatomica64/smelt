//! Runtime support for `node:sqlite`'s synchronous database, on `rusqlite`.
//!
//! What Node's `DatabaseSync`/`StatementSync` do, kept to the surface Smelt
//! models (`emitter::sqlite`):
//!
//! * a database is ONE connection shared by every handle on it, and `close()`
//!   closes it for all of them; a handle that was never opened (the
//!   placeholder a generated constructor fills a field with before assigning
//!   it) answers `database is not open`, Node's own message;
//! * a statement keeps its connection and its SQL, and is compiled once
//!   through the connection's statement cache — `prepare` compiles it eagerly
//!   so a syntax error throws there, as in Node;
//! * parameters bind at their own Rust type through `SmeltSqliteBind` (the
//!   generated code's `rusqlite::ToSql`), positionally, or by name when the
//!   only argument is a plain object (`{ id: 1 }` binds `:id`, `$id` or `@id`, prefix optional as
//!   Node's `allowBareNamedParameters` default allows); `null` binds NULL, a
//!   number binds an INTEGER when it is integral and safe, else a REAL, a
//!   string binds TEXT; anything else is Node's "cannot be bound" error —
//!   except `undefined`, which binds NULL here because a source `T | null`
//!   reaches the binder as an absent `Option<T>`, erased as `undefined`
//!   (Node throws for a genuine `undefined`; this is the one divergence);
//! * a row is a plain object keyed by column name: INTEGER and REAL read as
//!   numbers (an INTEGER beyond the safe range throws, as Node does without
//!   `readBigInts`), TEXT as a string, NULL as `null`, and a BLOB as a
//!   `Uint8Array` (the byte-view family's erased face, which is why a program
//!   using `node:sqlite` always carries that family: `stdlib::needs_byte_array_runtime`).

use crate::rust::CodeWriter;

/// The runtime types and their conversions.
const SQLITE_RUNTIME: &str = r#"/// A `node:sqlite` `DatabaseSync`: one connection shared by every handle.
#[derive(Clone)]
pub struct SmeltSqliteDatabase {
    /// The JavaScript object identity every handle on this database shares.
    id: usize,
    connection: ::std::rc::Rc<::std::cell::RefCell<Option<rusqlite::Connection>>>,
}

/// A database that was never opened: every operation answers "database is not open".
impl Default for SmeltSqliteDatabase {
    fn default() -> Self { Self { id: smelt_next_object_id(), connection: ::std::rc::Rc::default() } }
}

impl ::std::fmt::Debug for SmeltSqliteDatabase {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        formatter.debug_struct("SmeltSqliteDatabase").field("open", &self.connection.borrow().is_some()).finish()
    }
}

impl PartialEq for SmeltSqliteDatabase {
    fn eq(&self, other: &Self) -> bool { self.id == other.id }
}

/// An error thrown by a `node:sqlite` operation, carrying SQLite's message.
fn smelt_sqlite_error(message: impl ::std::fmt::Display) -> Box<dyn ::std::error::Error> {
    Box::new(::std::io::Error::other(message.to_string()))
}

#[allow(dead_code)]
impl SmeltSqliteDatabase {
    /// `new DatabaseSync(path)`; `":memory:"` is a private in-memory database.
    pub fn open(path: &str) -> Result<Self, Box<dyn ::std::error::Error>> {
        let connection = if path == ":memory:" { rusqlite::Connection::open_in_memory() } else { rusqlite::Connection::open(path) };
        let connection = connection.map_err(smelt_sqlite_error)?;
        Ok(Self { id: smelt_next_object_id(), connection: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(connection))) })
    }
    /// Run `action` on the open connection.
    fn with<R>(&self, action: impl FnOnce(&rusqlite::Connection) -> Result<R, Box<dyn ::std::error::Error>>) -> Result<R, Box<dyn ::std::error::Error>> {
        let connection = self.connection.borrow();
        let connection = connection.as_ref().ok_or_else(|| smelt_sqlite_error("database is not open"))?;
        action(connection)
    }
    /// `db.exec(sql)`: run every statement in `sql`.
    pub fn exec(&self, sql: &str) -> Result<(), Box<dyn ::std::error::Error>> {
        self.with(|connection| connection.execute_batch(sql).map_err(smelt_sqlite_error))
    }
    /// `db.prepare(sql)`: compile now, so a syntax error throws here.
    pub fn prepare(&self, sql: &str) -> Result<SmeltSqliteStatement, Box<dyn ::std::error::Error>> {
        self.with(|connection| connection.prepare_cached(sql).map(|_| ()).map_err(smelt_sqlite_error))?;
        Ok(SmeltSqliteStatement { id: smelt_next_object_id(), database: self.clone(), sql: ::std::rc::Rc::from(sql) })
    }
    /// `db.close()`: closes the connection for every handle on it.
    pub fn close(&self) -> Result<(), Box<dyn ::std::error::Error>> {
        let connection = self.connection.borrow_mut().take().ok_or_else(|| smelt_sqlite_error("database is not open"))?;
        connection.close().map_err(|(_, error)| smelt_sqlite_error(error))
    }
}

/// A `node:sqlite` `StatementSync`: its database and its SQL text.
#[derive(Clone)]
pub struct SmeltSqliteStatement {
    /// The JavaScript object identity of this statement.
    id: usize,
    database: SmeltSqliteDatabase,
    sql: ::std::rc::Rc<str>,
}

/// A statement on a database that was never opened.
impl Default for SmeltSqliteStatement {
    fn default() -> Self { Self { id: smelt_next_object_id(), database: SmeltSqliteDatabase::default(), sql: ::std::rc::Rc::from("") } }
}

impl ::std::fmt::Debug for SmeltSqliteStatement {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        formatter.debug_struct("SmeltSqliteStatement").field("sql", &self.sql).finish()
    }
}

impl PartialEq for SmeltSqliteStatement {
    fn eq(&self, other: &Self) -> bool { self.id == other.id }
}

/// A value a statement parameter can bind, at its own Rust type.
///
/// What `rusqlite::ToSql` is to a hand-written program: the generated call
/// passes each argument at the type the source gave it (`&title`, `&done`), and
/// only a value that is ALREADY erased binds through the `SmeltUnknown` impl.
pub trait SmeltSqliteBind {
    /// The SQLite value this binds as at 1-based `position`, or Node's error.
    fn smelt_sqlite_value(&self, position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>>;
    /// The value bound to the named parameter `name` (bare, no prefix), when
    /// this argument is a named-parameter object; `None` for any other value.
    fn smelt_sqlite_named(&self, _name: &str, _position: usize) -> Option<Result<rusqlite::types::Value, Box<dyn ::std::error::Error>>> { None }
    /// Whether this argument is a named-parameter object.
    fn smelt_sqlite_is_named(&self) -> bool { false }
}

/// Node's error for a value SQLite cannot store (a boolean, a function, ...).
fn smelt_sqlite_unbindable(position: usize) -> Box<dyn ::std::error::Error> {
    smelt_sqlite_error(format!("Provided value cannot be bound to SQLite parameter {position}."))
}

/// A number binds as an INTEGER when it is integral and safe, else a REAL.
impl SmeltSqliteBind for f64 {
    fn smelt_sqlite_value(&self, _position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>> {
        const SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
        Ok(if self.fract() == 0.0 && self.abs() <= SAFE_INTEGER { rusqlite::types::Value::Integer(*self as i64) } else { rusqlite::types::Value::Real(*self) })
    }
}

impl SmeltSqliteBind for i64 {
    fn smelt_sqlite_value(&self, _position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>> { Ok(rusqlite::types::Value::Integer(*self)) }
}

impl SmeltSqliteBind for String {
    fn smelt_sqlite_value(&self, _position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>> { Ok(rusqlite::types::Value::Text(self.clone())) }
}

/// Node refuses booleans; the program converts (`done ? 1 : 0`).
impl SmeltSqliteBind for bool {
    fn smelt_sqlite_value(&self, position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>> { Err(smelt_sqlite_unbindable(position)) }
}

/// An absent value binds NULL: a source `T | null` is carried as `Option<T>`.
impl<T: SmeltSqliteBind> SmeltSqliteBind for Option<T> {
    fn smelt_sqlite_value(&self, position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>> {
        match self { Some(value) => value.smelt_sqlite_value(position), None => Ok(rusqlite::types::Value::Null) }
    }
}

/// A string-keyed record is a named-parameter object (`{ id: 1 }`).
impl<V: SmeltSqliteBind + Clone> SmeltSqliteBind for SmeltRecord<String, V> {
    fn smelt_sqlite_value(&self, position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>> { Err(smelt_sqlite_unbindable(position)) }
    fn smelt_sqlite_named(&self, name: &str, position: usize) -> Option<Result<rusqlite::types::Value, Box<dyn ::std::error::Error>>> { self.get(name).map(|value| value.smelt_sqlite_value(position)) }
    fn smelt_sqlite_is_named(&self) -> bool { true }
}

/// An already-erased value binds by its run-time tag.
///
/// `undefined` binds NULL too: an absent `Option<T>` that was erased before it
/// reached the call arrives as `undefined`, indistinguishable from a genuine
/// one (Node throws for a genuine `undefined`; this is the one divergence).
impl SmeltSqliteBind for SmeltUnknown {
    fn smelt_sqlite_value(&self, position: usize) -> Result<rusqlite::types::Value, Box<dyn ::std::error::Error>> {
        match self {
            SmeltUnknown::Null | SmeltUnknown::Undefined => Ok(rusqlite::types::Value::Null),
            SmeltUnknown::Number(number) => number.smelt_sqlite_value(position),
            SmeltUnknown::String(text) => Ok(rusqlite::types::Value::Text(text.to_string())),
            _ => Err(smelt_sqlite_unbindable(position)),
        }
    }
    fn smelt_sqlite_named(&self, name: &str, position: usize) -> Option<Result<rusqlite::types::Value, Box<dyn ::std::error::Error>>> {
        match self { SmeltUnknown::Object(object) => object.get(name).map(|value| value.smelt_sqlite_value(position)), _ => None }
    }
    fn smelt_sqlite_is_named(&self) -> bool { matches!(self, SmeltUnknown::Object(_)) }
}

/// The JavaScript value of one column of a row.
fn smelt_sqlite_column(value: rusqlite::types::ValueRef<'_>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>> {
    const SAFE_INTEGER: i64 = 9_007_199_254_740_991;
    Ok(match value {
        rusqlite::types::ValueRef::Null => SmeltUnknown::Null,
        rusqlite::types::ValueRef::Integer(integer) if integer.abs() <= SAFE_INTEGER => SmeltUnknown::Number(integer as f64),
        rusqlite::types::ValueRef::Integer(integer) => {
            return Err(smelt_sqlite_error(format!("Value is too large to be represented as a JavaScript number: {integer}")));
        }
        rusqlite::types::ValueRef::Real(real) => SmeltUnknown::Number(real),
        rusqlite::types::ValueRef::Text(text) => SmeltUnknown::String(String::from_utf8_lossy(text).into()),
        rusqlite::types::ValueRef::Blob(bytes) => SmeltUint8Array::from_bytes(bytes.to_vec()).into_smelt_unknown(),
    })
}

#[allow(dead_code)]
impl SmeltSqliteStatement {
    /// Compile (through the cache), bind `parameters`, and hand the statement to `action`.
    fn with_bound<R>(&self, parameters: &[&dyn SmeltSqliteBind], action: impl FnOnce(&mut rusqlite::CachedStatement<'_>, &rusqlite::Connection) -> Result<R, Box<dyn ::std::error::Error>>) -> Result<R, Box<dyn ::std::error::Error>> {
        self.database.with(|connection| {
            let mut statement = connection.prepare_cached(&self.sql).map_err(smelt_sqlite_error)?;
            let count = statement.parameter_count();
            match parameters {
                [object] if object.smelt_sqlite_is_named() && count > 0 && statement.parameter_name(1).is_some() => {
                    for index in 1..=count {
                        let Some(name) = statement.parameter_name(index).map(str::to_owned) else {
                            return Err(smelt_sqlite_error(format!("Statement has an anonymous parameter {index}; pass parameters positionally.")));
                        };
                        let bare = name.trim_start_matches([':', '$', '@']);
                        let value = object.smelt_sqlite_named(bare, index).ok_or_else(|| smelt_sqlite_error(format!("Missing named parameter \"{bare}\"")))??;
                        statement.raw_bind_parameter(index, value).map_err(smelt_sqlite_error)?;
                    }
                }
                _ => {
                    for (offset, value) in parameters.iter().enumerate() {
                        let index = offset + 1;
                        if index > count {
                            return Err(smelt_sqlite_error(format!("column index out of range: {index}")));
                        }
                        statement.raw_bind_parameter(index, value.smelt_sqlite_value(index)?).map_err(smelt_sqlite_error)?;
                    }
                }
            }
            let result = action(&mut statement, connection);
            statement.clear_bindings();
            result
        })
    }
    /// Read every row of a bound statement as erased objects.
    ///
    /// The column names are read off the first STEPPED row, not the compiled
    /// statement: a cached statement whose table was altered since is
    /// re-prepared by SQLite on its first step, and only then reports the
    /// new columns (`SELECT *` after `ALTER TABLE .. ADD COLUMN`).
    fn rows(statement: &mut rusqlite::CachedStatement<'_>, limit: Option<usize>) -> Result<Vec<SmeltUnknown>, Box<dyn ::std::error::Error>> {
        let mut names: Option<Vec<String>> = None;
        let mut rows = statement.raw_query();
        let mut out = Vec::new();
        while limit.is_none_or(|limit| out.len() < limit) {
            let Some(row) = rows.next().map_err(smelt_sqlite_error)? else { break };
            let names = names.get_or_insert_with(|| row.as_ref().column_names().into_iter().map(str::to_owned).collect());
            let mut fields = Vec::with_capacity(names.len());
            for (index, name) in names.iter().enumerate() {
                fields.push((name.clone(), smelt_sqlite_column(row.get_ref(index).map_err(smelt_sqlite_error)?)?));
            }
            out.push(SmeltUnknown::Object(SmeltObject::new(fields)));
        }
        Ok(out)
    }
    /// `statement.all(...params)`: every row, each an erased object.
    pub fn smelt_sqlite_all(&self, parameters: &[&dyn SmeltSqliteBind]) -> Result<SmeltList<SmeltUnknown>, Box<dyn ::std::error::Error>> {
        self.with_bound(parameters, |statement, _| Self::rows(statement, None)).map(SmeltList::new)
    }
    /// `statement.get(...params)`: the first row, or `undefined`.
    pub fn smelt_sqlite_get(&self, parameters: &[&dyn SmeltSqliteBind]) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>> {
        self.with_bound(parameters, |statement, _| Self::rows(statement, Some(1)))
            .map(|rows| rows.into_iter().next().unwrap_or(SmeltUnknown::Undefined))
    }
    /// `statement.run(...params)`: `(changes, lastInsertRowid)`.
    pub fn run(&self, parameters: &[&dyn SmeltSqliteBind]) -> Result<(f64, f64), Box<dyn ::std::error::Error>> {
        self.with_bound(parameters, |statement, connection| {
            // A statement that returns rows still runs to completion, as in Node.
            let mut rows = statement.raw_query();
            while rows.next().map_err(smelt_sqlite_error)?.is_some() {}
            Ok((connection.changes() as f64, connection.last_insert_rowid() as f64))
        })
    }
}"#;

/// Emit the `node:sqlite` runtime and its erasure adapters.
///
/// Both types erase through the shared host-value adapter, retaining the live
/// value, so `const x: unknown = db` narrowed back reaches the same
/// connection. A record that did not come from an erasure recovers as a handle
/// that is not open — a real state of both types (the one `close()` leaves) —
/// rather than as a fabricated connection.
pub fn emit(writer: &mut CodeWriter) {
    for line in SQLITE_RUNTIME.lines() {
        writer.line(line);
    }
    writer.blank_line();
    crate::host_value_erasure::emit_adapters(
        writer,
        "SmeltSqliteDatabase",
        "__smelt_databasesync",
        &[],
        crate::host_value_erasure::Recovery::Empty("Self::default()"),
    );
    crate::host_value_erasure::emit_adapters(
        writer,
        "SmeltSqliteStatement",
        "__smelt_statementsync",
        &[],
        crate::host_value_erasure::Recovery::Empty("Self::default()"),
    );
}
