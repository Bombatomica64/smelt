//! Runtime class identity: which generated classes carry the hidden
//! `_smelt_class` tag naming the class an instance was constructed as.
//!
//! Smelt flattens inheritance: a subclass is its own struct carrying its base's
//! fields first (`classes::effective_class_fields`), and a value whose static
//! type is the base is the BASE struct. Passing a `Sub` where a `Base` is
//! expected (`function f(): Base { return new Sub(); }`, an element of a
//! `Base[]`) therefore converts it field-wise into a `Base`
//! (`emitter::coercion::record_adapter`). That conversion used to lose which
//! class the instance was constructed as, so `f() instanceof Sub` folded to
//! `false` from the static type alone, where JavaScript answers `true`: an
//! object's class is a property of the object, not of the slot holding it.
//!
//! The tag is that property. It is an `&'static str` field holding the source
//! name of the class whose constructor allocated the instance:
//!
//! - every struct literal that ALLOCATES an instance (a constructor's `this`)
//!   writes its own class's name;
//! - every conversion between two classes of one hierarchy copies the source's
//!   tag, so an upcast keeps the constructed class;
//! - an erasure stamps the tag as the `__smelt_class` marker, and narrowing an
//!   erased value back restores it from that marker;
//! - `value instanceof Sub` on a value statically typed as a strict ancestor of
//!   `Sub` compares the tag with `Sub` and its descendants
//!   (`emitter::instance_of`).
//!
//! Pay-for-use: a class carries the tag only when its hierarchy is the
//! hierarchy of some `instanceof` target that has a program base class, i.e.
//! only where a value statically typed as an ancestor can be asked whether it
//! is a subclass. Every other class keeps its exact previous emission.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the identity helpers are shared by the struct emitters and the function emitter"
)]

use std::collections::HashSet;
use std::fmt::Write as _;

use smelt_hir::Symbol;
use smelt_mir::{Mir, MirClass, Rvalue, Statement};

use crate::classes::find_class;

/// The Rust name of the hidden identity field.
///
/// Not a source identifier a class can declare (`_smelt` names are reserved
/// for generated members, see `names::escape`), and never an own property: no
/// enumeration, JSON or inspection view lists it.
pub(crate) const CLASS_IDENTITY_FIELD: &str = "_smelt_class";

/// The root of `class`'s hierarchy inside the program: the topmost ancestor
/// that is a generated class (a builtin base such as `Error` ends the walk).
fn hierarchy_root(mir: &Mir, class: Symbol) -> Symbol {
    let mut current = class;
    let mut seen = HashSet::from([class]);
    while let Some(base) = find_class(mir, current).and_then(|found| found.base) {
        if find_class(mir, base).is_none() || !seen.insert(base) {
            break;
        }
        current = base;
    }
    current
}

/// Whether `class` is an instance class of the program (a statics frame is
/// the class VALUE of a per-evaluation class, never an instance a hierarchy
/// check meets).
fn is_instance_class(class: &MirClass) -> bool {
    class.statics_of.is_none()
}

/// The classes whose instances carry the identity tag.
///
/// A class carries it when its hierarchy (all classes sharing its program
/// root) contains an `instanceof` target that extends another generated
/// class: only then can a value whose static type is an ancestor of the
/// target hold an instance of the target.
pub(crate) fn identity_classes(mir: &Mir) -> HashSet<Symbol> {
    let mut roots = HashSet::new();
    let bodies = mir
        .functions
        .iter()
        .map(|function| &function.blocks)
        .chain(mir.closures.iter().map(|closure| &closure.blocks));
    for blocks in bodies {
        for block in blocks {
            for statement in &block.statements {
                let Statement::Assign {
                    value: Rvalue::InstanceOf { class, .. },
                    ..
                } = statement
                else {
                    continue;
                };
                let Some(target) = find_class(mir, *class) else {
                    continue;
                };
                if is_instance_class(target)
                    && target
                        .base
                        .is_some_and(|base| find_class(mir, base).is_some())
                {
                    roots.insert(hierarchy_root(mir, *class));
                }
            }
        }
    }
    if roots.is_empty() {
        return HashSet::new();
    }
    mir.classes
        .iter()
        .filter(|class| is_instance_class(class))
        .filter(|class| roots.contains(&hierarchy_root(mir, class.name)))
        .map(|class| class.name)
        .collect()
}

/// The Rust string literal a tag of `class` holds: the class's source name,
/// which is also what its erasure's `__smelt_class` marker carries and what
/// `instanceof` compares.
pub(crate) fn identity_literal(mir: &Mir, class: Symbol) -> String {
    let name = mir
        .names
        .get(class)
        .or_else(|| mir.symbols.get(class))
        .unwrap_or("class");
    format!("{name:?}")
}

/// The source names a tag may hold for an instance that is `target` or one of
/// its descendants: the set `value instanceof target` accepts.
pub(crate) fn identity_literals_extending(mir: &Mir, target: Symbol) -> Vec<String> {
    let mut literals = Vec::new();
    for class in &mir.classes {
        if !is_instance_class(class) {
            continue;
        }
        let mut current = Some(class.name);
        let mut seen = HashSet::new();
        while let Some(symbol) = current {
            if symbol == target {
                let literal = identity_literal(mir, class.name);
                if !literals.contains(&literal) {
                    literals.push(literal);
                }
                break;
            }
            if !seen.insert(symbol) {
                break;
            }
            current = find_class(mir, symbol).and_then(|found| found.base);
        }
    }
    literals
}

/// The struct field line declaring the tag. A class that also derives serde
/// skips it: the tag is not an own property, so `JSON.stringify` never sees
/// it, and a decoded instance takes its class's own name from `Default`.
pub(crate) fn field_declaration(serde: bool) -> String {
    let skip = if serde { "#[serde(skip)] " } else { "" };
    format!("{skip}{CLASS_IDENTITY_FIELD}: &'static str,")
}

/// The tag of an instance of `class` rebuilt from an erased object, whose
/// `__smelt_class` marker (`marker`, an `Option<SmeltUnknown>` expression)
/// names the class it was constructed as: that class when it is `class` or a
/// descendant, otherwise `class` itself (a plain object narrowed to the class).
pub(crate) fn identity_from_marker_expr(mir: &Mir, class: Symbol, marker: &str) -> String {
    let own = identity_literal(mir, class);
    let mut arms = String::new();
    for literal in identity_literals_extending(mir, class) {
        if literal != own {
            let _ = write!(
                arms,
                "Some(SmeltUnknown::String(smelt_class_name)) if smelt_class_name.eq_str({literal}) => {literal}, "
            );
        }
    }
    if arms.is_empty() {
        return own;
    }
    format!("match {marker} {{ {arms}_ => {own} }}")
}
