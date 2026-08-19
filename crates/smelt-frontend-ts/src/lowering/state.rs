//! Owned sub-states extracted out of the `ModuleBuilder` god object.
//!
//! Each module here holds one cohesive field group behind a small struct with
//! named operations. `ModuleBuilder` keeps a field per group instead of a field
//! per map, so an invariant that used to span several loose maps becomes a
//! method on the struct that owns them.
//!
//! See `docs/architecture-plan-second-pass.md`, finding C.

pub(in crate::lowering) mod class_registry;
pub(in crate::lowering) mod interface_registry;
pub(in crate::lowering) mod local_scope;
