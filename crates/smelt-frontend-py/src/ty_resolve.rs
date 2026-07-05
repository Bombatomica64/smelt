//! Optional `ty`-backed type resolution seam for the Python frontend.
//!
//! The Python frontend historically required explicit type annotations and
//! emitted a hard error otherwise. Issue #93 replaces those errors, for
//! resolvable cases, with types inferred by Astral's `ty` (see
//! `smelt-py-types`). Because the `ty` dependency tree is heavy, that
//! resolution lives behind the crate's optional `ty` feature.
//!
//! This module is the single seam between the lowering walk and `ty`. It
//! exposes one type, [`ResolvedTypes`], with the same public surface in both
//! build configurations:
//!
//! * With `--features ty`, it wraps `smelt_py_types::ResolvedModuleTypes` and
//!   answers return/parameter type queries keyed by AST byte offset.
//! * Without the feature, it is a zero-sized stub whose queries always return
//!   `None`, so the frontend falls back to its annotation-based lowering and
//!   the original "must have an explicit ... annotation" errors still fire.
//!
//! Keeping the `#[cfg]` split here means the lowering call sites are written
//! once, against a stable API, regardless of whether `ty` is compiled in.
#![allow(
    clippy::redundant_pub_crate,
    reason = "the crate root exposes this seam to the lowering builder"
)]

use ruff_text_size::TextSize;

/// Types resolved by `ty` for the module currently being lowered.
///
/// Lookups are keyed by the source byte offset of the AST node the frontend is
/// visiting (a function-definition start for a return type, a parameter start
/// for a parameter type). Both accessors return the resolved type as a
/// canonical Python type *spelling* (e.g. `"int"`, `"list[int]"`,
/// `"str | None"`) that the frontend re-parses through its annotation lowering.
#[derive(Debug, Default)]
pub(crate) struct ResolvedTypes {
    /// `ty`'s resolved module types, or `None` if resolution was skipped/failed.
    #[cfg(feature = "ty")]
    inner: Option<smelt_py_types::ResolvedModuleTypes>,
}

impl ResolvedTypes {
    /// A resolver that resolves nothing (feature off, or resolution skipped).
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self::default()
    }

    /// Resolve every function/method return and parameter type in `source`
    /// via `ty`.
    ///
    /// `stem` is the file stem used for `ty`'s temporary module (only affects
    /// `ty`'s module naming, not lookup keys). If `ty` fails to run over the
    /// source, resolution is silently skipped and every later query returns
    /// `None`, so lowering falls back to annotations.
    ///
    /// Without the `ty` feature this is a no-op returning a disabled resolver.
    #[must_use]
    pub(crate) fn resolve(source: &str, stem: &str) -> Self {
        #[cfg(feature = "ty")]
        {
            let inner = smelt_py_types::resolve_module_types(source, stem).ok();
            Self { inner }
        }
        #[cfg(not(feature = "ty"))]
        {
            let _ = (source, stem);
            Self::disabled()
        }
    }

    /// Return the `ty`-resolved return-type spelling for the function whose
    /// definition starts at `offset`, if any.
    ///
    /// `offset` is this crate's own `ruff_text_size::TextSize` (from the
    /// crates.io Ruff parser). It is converted to a plain `u32` before crossing
    /// into `smelt_py_types`, whose git-pinned Ruff `TextSize` is a *different*
    /// crate and would not type-check here. See [`ResolvedTypes`] docs.
    #[must_use]
    pub(crate) fn return_type_at(&self, offset: TextSize) -> Option<&str> {
        #[cfg(feature = "ty")]
        {
            self.inner.as_ref()?.return_type_at(offset.to_u32())
        }
        #[cfg(not(feature = "ty"))]
        {
            let _ = (self, offset);
            None
        }
    }

    /// Return the `ty`-resolved parameter-type spelling for the parameter whose
    /// declaration starts at `offset`, if any.
    ///
    /// `offset` is converted to a plain `u32` before crossing into
    /// `smelt_py_types`; see [`Self::return_type_at`] for why.
    #[must_use]
    pub(crate) fn param_type_at(&self, offset: TextSize) -> Option<&str> {
        #[cfg(feature = "ty")]
        {
            self.inner.as_ref()?.param_type_at(offset.to_u32())
        }
        #[cfg(not(feature = "ty"))]
        {
            let _ = (self, offset);
            None
        }
    }
}
