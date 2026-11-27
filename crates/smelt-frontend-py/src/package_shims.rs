//! Third-party Python definitions the frontend recognizes, at one boundary.
//!
//! The general lowering rules never test a package's spelling. When a
//! third-party definition needs a decision of its own, it is listed here, by the
//! fully qualified name an expression RESOLVES to ([`crate::name_resolution`]),
//! never by how the use site spells it: `models.Model` after
//! `from django.db import models`, `Base` after
//! `from django.db.models import Model as Base`, and `django.db.models.Model`
//! all denote the same entry, while a module's own `models.Model` (a local
//! class, or an unrelated `models` package) denotes something else and is
//! lowered like any other base.
//!
//! The rule the table feeds is general: a class whose base is a framework class
//! with a metaclass or descriptor protocol Smelt cannot express is refused with
//! a diagnostic that names the framework, instead of lowering into a class that
//! silently lacks the framework's behaviour. Adding a framework is a row here,
//! not a check in lowering.
#![allow(
    clippy::redundant_pub_crate,
    reason = "the table is read by sibling lowering modules and the crate's error constructors"
)]

/// A third-party base class whose runtime machinery Smelt cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnsupportedFrameworkBase {
    /// The definition's fully qualified name (`module.attribute`).
    pub(crate) qualified_name: &'static str,
    /// The framework, as a diagnostic names it.
    pub(crate) framework: &'static str,
    /// Why a subclass cannot be lowered, shown as the diagnostic's note.
    pub(crate) reason: &'static str,
}

/// Why a Django model cannot be lowered.
const DJANGO_MODEL_REASON: &str =
    "Django's Model metaclass and descriptor protocol cannot be expressed in smelt HIR.";

/// Every framework base class the frontend refuses, by qualified name.
///
/// `django.db.models.Model` is the public spelling; `django.db.models.base`
/// is the module that defines it, which a `from django.db.models.base import
/// Model` reaches directly.
const UNSUPPORTED_FRAMEWORK_BASES: &[UnsupportedFrameworkBase] = &[
    UnsupportedFrameworkBase {
        qualified_name: "django.db.models.Model",
        framework: "Django ORM",
        reason: DJANGO_MODEL_REASON,
    },
    UnsupportedFrameworkBase {
        qualified_name: "django.db.models.base.Model",
        framework: "Django ORM",
        reason: DJANGO_MODEL_REASON,
    },
];

/// The unsupported framework base a resolved qualified name denotes, if any.
pub(crate) fn unsupported_framework_base(
    qualified_name: &str,
) -> Option<&'static UnsupportedFrameworkBase> {
    UNSUPPORTED_FRAMEWORK_BASES
        .iter()
        .find(|base| base.qualified_name == qualified_name)
}

#[cfg(test)]
mod tests {
    use super::unsupported_framework_base;

    #[test]
    fn a_framework_base_is_found_by_its_qualified_name_only() {
        assert!(unsupported_framework_base("django.db.models.Model").is_some());
        assert!(unsupported_framework_base("django.db.models.base.Model").is_some());
        // The spelling a use site might have is not a key.
        assert!(unsupported_framework_base("models.Model").is_none());
        assert!(unsupported_framework_base("myapp.models.Model").is_none());
    }
}
