//! Where each closure is constructed, derived from the bodies.
//!
//! A [`crate::MirClosure`] is built by exactly one [`Rvalue::Closure`], and
//! that construction's `captures` operands are the authoritative record of what
//! it captures from which local of which body. The closure table also keeps a
//! `source_local` per capture because the backend reads it everywhere, so the
//! two copies must agree after any pass that rewrites operands.
//!
//! The site is therefore the source and `source_local` the derived copy:
//! [`sync_capture_sources`] rewrites every `source_local` from its site, and
//! [`crate::opt::optimize`] runs it after every pass. The validator checks
//! that they agree, so a pass that leaves them apart fails validation.

use crate::{BodyOwner, ClosureId, Mir, Operand, Rvalue, Statement};

/// One closure construction: the body it is in and the operands it captures.
#[derive(Debug, Clone, Copy)]
pub struct ClosureSite<'a> {
    /// The body holding the `Rvalue::Closure`.
    pub owner: BodyOwner,
    /// Its captured operands, in capture order.
    pub captures: &'a [Operand],
}

/// What a scan found for one closure id.
#[derive(Debug, Clone, Copy)]
pub enum SiteLookup<'a> {
    /// No body constructs the closure (its construction was removed as dead).
    Unconstructed,
    /// Exactly one body constructs it, once.
    One(ClosureSite<'a>),
    /// More than one construction: the table entry is shared, which no
    /// lowering produces and the validator rejects.
    Many,
}

impl Mir {
    /// The construction site of every closure, indexed by [`ClosureId`].
    #[must_use]
    pub fn closure_sites(&self) -> Vec<SiteLookup<'_>> {
        let mut sites = vec![SiteLookup::Unconstructed; self.closures.len()];
        for body in self.bodies() {
            for block in body.blocks {
                for statement in &block.statements {
                    let value = match statement {
                        Statement::Assign { value, .. }
                        | Statement::AssignPlace { value, .. }
                        | Statement::DictEntryUpdate { value, .. } => value,
                    };
                    let Rvalue::Closure { id, captures } = value else {
                        continue;
                    };
                    let Some(slot) = sites.get_mut(id.index()) else {
                        continue;
                    };
                    *slot = match slot {
                        SiteLookup::Unconstructed => {
                            SiteLookup::One(ClosureSite { owner: body.owner, captures })
                        }
                        SiteLookup::One(_) | SiteLookup::Many => SiteLookup::Many,
                    };
                }
            }
        }
        sites
    }
}

/// A closure and the local each of its captures' site operands reads.
type DerivedSources = (ClosureId, Vec<Option<crate::LocalId>>);

/// Rewrite each capture's `source_local` to the local its construction site
/// reads, so the table entry agrees with the site.
///
/// A capture whose site operand is not a plain local (or whose closure has no
/// single site) is left alone; the validator reports it.
pub fn sync_capture_sources(mir: &mut Mir) {
    let derived: Vec<DerivedSources> = mir
        .closure_sites()
        .into_iter()
        .zip(&mir.closures)
        .filter_map(|(lookup, closure)| match lookup {
            SiteLookup::One(site) => Some((
                closure.id,
                site.captures.iter().map(Operand::local).collect(),
            )),
            SiteLookup::Unconstructed | SiteLookup::Many => None,
        })
        .collect();
    for (id, locals) in derived {
        let Some(closure) = mir.closures.get_mut(id.index()) else {
            continue;
        };
        for (capture, derived_local) in closure.captures.iter_mut().zip(locals) {
            if let Some(local) = derived_local {
                capture.source_local = local;
            }
        }
    }
}
