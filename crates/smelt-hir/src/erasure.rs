//! Erasure reasons, recorded where lowering erases a value to `Type::Unknown`.
//!
//! The tagged runtime value is emitted as `SmeltUnknown`.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), `src/sema/aot_rules.rs`:
//! a closed table of *rules* (here: [`ErasureReason`]), each with a stable
//! kebab-case name, a one-line help text and a default [`ErasureSeverity`]
//! (`allow`/`warn`/`deny`), configured per reason and per path by the CLI and
//! the manifest.
//!
//! # Why a decision-time record
//!
//! `Type::Unknown` is interned: every erased value shares one `TypeId`, so once
//! lowering returns, *why* a value was erased is gone. The `SmeltUnknown`
//! report can only classify generated Rust TEXT after the fact. Each erasure
//! site therefore appends an [`ErasureRecord`] — reason, source span, detail —
//! to [`crate::Crate::erasures`] at the moment it chooses `Type::Unknown`, so a
//! report can say which source line erased a value and why.
//!
//! # Legitimate vs avoidable
//!
//! Every reason maps to exactly one [`ErasureClass`] (see
//! [`ErasureReason::class`]), mirroring the `SmeltUnknown boundaries` policy:
//! a value whose SOURCE type is dynamic (`unknown`, an open layout, data from
//! outside the type system, an untyped package, a value inspected through
//! runtime narrowing, a test matcher's dynamic comparison) is a legitimate
//! boundary; a value whose static shape exists in source but that Smelt failed
//! to carry (`any`, a type-level construct Smelt does not evaluate, a missing
//! inference, an erased generic or union in codegen, an unclassified site) is
//! avoidable.

use serde::{Deserialize, Serialize};

use crate::ids::Span;

/// Why a lowering pass erased a value's static type to `Type::Unknown`.
///
/// The variants are Smelt's real erasure causes, grouped by the pass and the
/// source construct that decides them. Names are stable: they appear in
/// `Smelt.toml` (`[erasure]`), on the CLI (`--erasure <reason>=<level>`), in
/// diagnostics (`warning[erasure/<reason>]`) and in the `smelt-erasure.json`
/// sidecar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ErasureReason {
    /// The source annotation is `unknown`: the author declared a dynamic value.
    SourceUnknown,
    /// The source annotation is `any` (or `any[]`, `Function`-as-any): the
    /// author opted out of typing, so Smelt has no shape to carry.
    ExplicitAny,
    /// A generic type argument was left unfilled or defaulted to `any`, so the
    /// instantiation has no concrete argument to substitute.
    UnfilledGeneric,
    /// A type-level construct Smelt does not evaluate: conditional types,
    /// indexed access `T[K]`, mapped types, `this` outside a class,
    /// `ReturnType`/`Parameters` of a non-function, intersection fallback,
    /// mixed-arity function unions.
    TypeLevel,
    /// A source type Smelt does not model at all (`symbol`, callable-object
    /// surfaces), so the value travels erased.
    UnmodeledType,
    /// The declared type admits objects of any layout: an index signature or
    /// record whose value type is open, an empty object type, `object`.
    /// Bun's `open-layout`.
    OpenLayout,
    /// No annotation and no successful inference: a parameter, local, field or
    /// return whose type Smelt could not recover.
    MissingAnnotation,
    /// A builtin or stdlib call whose result TypeScript types precisely but
    /// whose result Smelt does not model yet, so it returns the tagged value.
    UntypedBuiltin,
    /// Data from outside the type system: `JSON.parse` without a typed target,
    /// a request/response body, a host stdlib call that returns dynamic data.
    /// Bun's `external-type`.
    ExternalType,
    /// A value imported from a module Smelt does not have typed (an untyped
    /// package or a modeled module without a declared export). Bun's
    /// `untyped-package`.
    UntypedPackage,
    /// A read of a global, `this`, `require`, or a member of a value whose
    /// layout is unknown, resolved by name at runtime. Bun's `by-name`.
    ByName,
    /// An operator whose result is erased because an operand is erased or the
    /// operator's result type is not static (`+` over erased operands, `in`,
    /// computed member writes).
    DynamicOperator,
    /// A value inspected through runtime narrowing (a type guard, `typeof`,
    /// `instanceof` on an erased value), so the check happens at runtime.
    /// Bun's `can-throw`.
    RuntimeNarrowing,
    /// A test-harness value (`expect(...)`, asymmetric matchers) compared
    /// dynamically by the test runtime.
    TestHarness,
    /// Python dynamic metadata (`__dict__`, `getattr` on an untyped object):
    /// the source language itself makes the value dynamic.
    DynamicMetadata,
    /// Codegen: a type parameter resolved to erased (no Rust generic carries
    /// it at this site).
    ErasedGeneric,
    /// Codegen: a union that is not a concrete generated union enum.
    ErasedUnion,
    /// Codegen: a `Type::Class` naming no lowered class.
    UnresolvedClass,
    /// Codegen: a `never` value rendered as the tagged value.
    NeverValue,
    /// An erasure site no rule above classifies. Reported (default `warn`),
    /// never silently dropped: it marks a site that still needs a reason.
    Unclassified,
}

/// Whether an erasure is a genuine dynamic boundary or avoidable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ErasureClass {
    /// The source type itself is dynamic; erasure is the faithful lowering.
    Legitimate,
    /// The source carries a static shape that Smelt failed to preserve.
    Avoidable,
}

impl ErasureClass {
    /// Stable kebab-case identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legitimate => "legitimate",
            Self::Avoidable => "avoidable",
        }
    }
}

/// How a reported erasure is surfaced. Bun's `Severity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ErasureSeverity {
    /// Recorded in the sidecar only; nothing is printed.
    Allow,
    /// Printed as `warning[erasure/<reason>]`; the build continues.
    Warn,
    /// Printed as `error[erasure/<reason>]`; `build`/`check` fail.
    Deny,
}

impl ErasureSeverity {
    /// Stable lowercase identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Deny => "deny",
        }
    }

    /// Parse a severity name (`allow`, `warn`, `deny`).
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "allow" => Some(Self::Allow),
            "warn" => Some(Self::Warn),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// Static description of one reason: name, class, default severity, help.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ErasureReasonInfo {
    /// The reason described.
    pub reason: ErasureReason,
    /// Stable kebab-case name.
    pub name: &'static str,
    /// Diagnostic code, `erasure/<name>`.
    pub code: &'static str,
    /// Legitimate boundary or avoidable erasure.
    pub class: ErasureClass,
    /// Severity when neither the manifest nor the CLI sets one.
    pub default: ErasureSeverity,
    /// One-line explanation shown with diagnostics.
    pub help: &'static str,
}

/// Every reason, in declaration order. Indexed by `ErasureReason as usize`.
///
/// Defaults follow Bun's table, where every rule but one is `allow`: erasure
/// has always been silent in Smelt and the `SmeltUnknown` report already
/// ratchets the totals, so warnings are opt-in per project. The exception is
/// [`ErasureReason::Unclassified`] (`warn`), the analogue of Bun's
/// `untyped-file`: it means Smelt erased a value without knowing why, which is
/// a Smelt gap a user should see rather than a choice they made.
pub static ERASURE_REASONS: [ErasureReasonInfo; ErasureReason::COUNT] = [
    info(ErasureReason::SourceUnknown, "source-unknown", "erasure/source-unknown", ErasureClass::Legitimate, ErasureSeverity::Allow,
        "the source declares the value `unknown`, so it is carried as a tagged runtime value"),
    info(ErasureReason::ExplicitAny, "explicit-any", "erasure/explicit-any", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "the source annotates the value `any`, so Smelt has no static shape to carry; annotate the real type"),
    info(ErasureReason::UnfilledGeneric, "unfilled-generic", "erasure/unfilled-generic", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "a generic type argument is unfilled or defaults to `any`; pass the type argument explicitly"),
    info(ErasureReason::TypeLevel, "type-level", "erasure/type-level", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "a type-level construct (conditional, indexed access, mapped type, ...) that Smelt does not evaluate"),
    info(ErasureReason::UnmodeledType, "unmodeled-type", "erasure/unmodeled-type", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "a source type Smelt does not model (`symbol`, callable-object surfaces)"),
    info(ErasureReason::OpenLayout, "open-layout", "erasure/open-layout", ErasureClass::Legitimate, ErasureSeverity::Allow,
        "the declared type admits objects of any layout (index signature, empty object type, `object`)"),
    info(ErasureReason::MissingAnnotation, "missing-annotation", "erasure/missing-annotation", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "no annotation and no successful inference; annotate the parameter, local, field or return"),
    info(ErasureReason::UntypedBuiltin, "untyped-builtin", "erasure/untyped-builtin", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "a builtin whose result type TypeScript knows but Smelt does not model yet"),
    info(ErasureReason::ExternalType, "external-type", "erasure/external-type", ErasureClass::Legitimate, ErasureSeverity::Allow,
        "data from outside the type system (JSON.parse, a request or response body, a host call)"),
    info(ErasureReason::UntypedPackage, "untyped-package", "erasure/untyped-package", ErasureClass::Legitimate, ErasureSeverity::Allow,
        "a value imported from a module Smelt has no types for"),
    info(ErasureReason::ByName, "by-name", "erasure/by-name", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "a global, `this`, `require` or a member of a value of unknown layout, resolved by name"),
    info(ErasureReason::DynamicOperator, "dynamic-operator", "erasure/dynamic-operator", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "an operator over an erased operand, or one whose result type is not static"),
    info(ErasureReason::RuntimeNarrowing, "runtime-narrowing", "erasure/runtime-narrowing", ErasureClass::Legitimate, ErasureSeverity::Allow,
        "a value inspected through runtime narrowing (type guard, typeof, instanceof)"),
    info(ErasureReason::TestHarness, "test-harness", "erasure/test-harness", ErasureClass::Legitimate, ErasureSeverity::Allow,
        "a test-harness value compared dynamically by the test runtime"),
    info(ErasureReason::DynamicMetadata, "dynamic-metadata", "erasure/dynamic-metadata", ErasureClass::Legitimate, ErasureSeverity::Allow,
        "Python dynamic metadata the source language itself makes dynamic"),
    info(ErasureReason::ErasedGeneric, "erased-generic", "erasure/erased-generic", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "codegen erased a type parameter that no Rust generic carries at this site"),
    info(ErasureReason::ErasedUnion, "erased-union", "erasure/erased-union", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "codegen erased a union that is not a concrete generated union"),
    info(ErasureReason::UnresolvedClass, "unresolved-class", "erasure/unresolved-class", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "codegen erased a class type that names no lowered class"),
    info(ErasureReason::NeverValue, "never-value", "erasure/never-value", ErasureClass::Avoidable, ErasureSeverity::Allow,
        "codegen rendered a `never` value as the tagged runtime value"),
    info(ErasureReason::Unclassified, "unclassified", "erasure/unclassified", ErasureClass::Avoidable, ErasureSeverity::Warn,
        "Smelt erased this value at a site that records no reason yet; please report it"),
];

/// Build one [`ErasureReasonInfo`] row (keeps the table one line per field).
#[expect(
    clippy::too_many_arguments,
    reason = "one positional argument per table column keeps each row on two lines"
)]
const fn info(
    reason: ErasureReason,
    name: &'static str,
    code: &'static str,
    class: ErasureClass,
    default: ErasureSeverity,
    help: &'static str,
) -> ErasureReasonInfo {
    ErasureReasonInfo {
        reason,
        name,
        code,
        class,
        default,
        help,
    }
}

// `ERASURE_REASONS` is indexed by `ErasureReason as usize`.
const _: () = {
    let mut at = 0;
    while at < ERASURE_REASONS.len() {
        assert!(
            ERASURE_REASONS[at].reason as usize == at,
            "ERASURE_REASONS is indexed by ErasureReason"
        );
        at += 1;
    }
};

impl ErasureReason {
    /// Number of reasons.
    pub const COUNT: usize = 20;

    /// The static description of this reason.
    #[must_use]
    pub fn info(self) -> &'static ErasureReasonInfo {
        &ERASURE_REASONS[self as usize]
    }

    /// Stable kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// Legitimate boundary or avoidable erasure.
    #[must_use]
    pub fn class(self) -> ErasureClass {
        self.info().class
    }

    /// Default severity when nothing configures this reason.
    #[must_use]
    pub fn default_severity(self) -> ErasureSeverity {
        self.info().default
    }

    /// Look a reason up by its kebab-case name.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        ERASURE_REASONS
            .iter()
            .find(|info| info.name == name)
            .map(|info| info.reason)
    }

    /// Every reason in declaration order.
    pub fn all() -> impl Iterator<Item = Self> {
        ERASURE_REASONS.iter().map(|info| info.reason)
    }
}

/// Sort key of an [`ErasureRecord`]: file, start, end, reason, detail.
type RecordKey<'a> = (u32, u32, u32, ErasureReason, &'a str);

/// One erasure decision: why, where, and a short detail.
///
/// `span` is the innermost source node the deciding pass was lowering (a type
/// annotation, an expression or a statement; for codegen-side reasons, the
/// owning function's span when nothing finer is available).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ErasureRecord {
    /// Why the value was erased.
    pub reason: ErasureReason,
    /// Where the decision was made.
    pub span: Span,
    /// Short free-form detail (the construct, the name), may be empty.
    pub detail: String,
}

impl ErasureRecord {
    /// Build a record.
    #[must_use]
    pub fn new(reason: ErasureReason, span: Span, detail: impl Into<String>) -> Self {
        Self {
            reason,
            span,
            detail: detail.into(),
        }
    }

    /// Sort key: file, start, end, reason, detail.
    fn key(&self) -> RecordKey<'_> {
        (
            self.span.file.0,
            self.span.start,
            self.span.end,
            self.reason,
            self.detail.as_str(),
        )
    }
}

/// Sort records by file/position/reason and drop exact duplicates.
///
/// Lowering may visit one source node more than once (specialization replays,
/// probes that re-lower a type), which would otherwise record the same
/// decision twice.
pub fn normalize_erasures(records: &mut Vec<ErasureRecord>) {
    records.sort_by(|left, right| left.key().cmp(&right.key()));
    records.dedup_by(|left, right| left.key() == right.key());
}

/// Resolve a byte offset in `source` to a 1-based `(line, column)` pair.
///
/// The column counts UTF-8 bytes from the line start, matching the manifest
/// diagnostics. Returns `None` for an offset past the end of `source` or one
/// that is not on a character boundary (a span from synthesized code).
#[must_use]
pub fn line_col(source: &str, offset: u32) -> Option<(u32, u32)> {
    let offset = usize::try_from(offset).ok()?;
    let prefix = source.get(..offset)?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = offset - line_start + 1;
    Some((u32::try_from(line).ok()?, u32::try_from(column).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::FileId;

    /// Every reason round-trips through its name and serde spelling.
    #[test]
    fn reason_names_round_trip() {
        for reason in ErasureReason::all() {
            assert_eq!(ErasureReason::named(reason.name()), Some(reason));
            assert_eq!(reason.info().code, format!("erasure/{}", reason.name()));
            let json = serde_json_like(reason);
            assert_eq!(json, reason.name());
        }
        assert_eq!(ErasureReason::all().count(), ErasureReason::COUNT);
    }

    /// The legitimate/avoidable split is the documented policy: source-dynamic
    /// reasons are legitimate, Smelt-side failures are avoidable. A reason moved
    /// to the wrong class fails here.
    #[test]
    fn reason_classes_follow_policy() {
        let legitimate = [
            ErasureReason::SourceUnknown,
            ErasureReason::OpenLayout,
            ErasureReason::ExternalType,
            ErasureReason::UntypedPackage,
            ErasureReason::RuntimeNarrowing,
            ErasureReason::TestHarness,
            ErasureReason::DynamicMetadata,
        ];
        for reason in ErasureReason::all() {
            let expected = if legitimate.contains(&reason) {
                ErasureClass::Legitimate
            } else {
                ErasureClass::Avoidable
            };
            assert_eq!(reason.class(), expected, "{}", reason.name());
        }
        assert_eq!(ErasureReason::ExplicitAny.class(), ErasureClass::Avoidable);
        assert_eq!(ErasureReason::Unclassified.default_severity(), ErasureSeverity::Warn);
    }

    /// Normalization sorts by position and removes duplicates.
    #[test]
    fn normalize_sorts_and_dedups() {
        let at = |start| Span::new(FileId(0), start, start + 1);
        let mut records = vec![
            ErasureRecord::new(ErasureReason::ExplicitAny, at(9), ""),
            ErasureRecord::new(ErasureReason::ExplicitAny, at(2), ""),
            ErasureRecord::new(ErasureReason::ExplicitAny, at(9), ""),
        ];
        normalize_erasures(&mut records);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].span.start, 2);
    }

    /// Byte offsets resolve to 1-based line and column.
    #[test]
    fn line_col_is_one_based() {
        let source = "ab\ncd\n";
        assert_eq!(line_col(source, 0), Some((1, 1)));
        assert_eq!(line_col(source, 4), Some((2, 2)));
        assert_eq!(line_col(source, 99), None);
    }

    /// The serde spelling of a reason (without a JSON dependency).
    fn serde_json_like(reason: ErasureReason) -> String {
        format!("{reason:?}")
            .chars()
            .enumerate()
            .flat_map(|(index, ch)| {
                let lower = ch.to_ascii_lowercase();
                if ch.is_ascii_uppercase() && index > 0 {
                    vec!['-', lower]
                } else {
                    vec![lower]
                }
            })
            .collect()
    }
}
