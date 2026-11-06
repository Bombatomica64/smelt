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
//!   using `node:sqlite` always carries that family: `Need::ByteArray`).

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

