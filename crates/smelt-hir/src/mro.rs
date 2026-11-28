//! Method resolution order: CPython's C3 linearization of a class's bases.
//!
//! A class's MRO is the class itself followed by the merge of its bases' MROs
//! and the list of its bases: repeatedly take the first head (first element of
//! a list) that appears in no list's tail, and remove it from every list. With
//! one base the MRO is the class followed by its base's MRO, which is the base
//! chain every single-inheritance consumer walks. See
//! `docs/design/python-multiple-inheritance.md`.

use std::collections::HashSet;

use crate::Symbol;

/// Why a class has no linearization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MroError {
    /// The class is its own ancestor.
    Cycle(Symbol),
    /// The bases' orders conflict (CPython: "Cannot create a consistent
    /// method resolution order"); the payload is the heads left unmerged.
    Inconsistent(Vec<Symbol>),
}

/// The C3 linearization of `class`, whose program bases (first base first)
/// `bases_of` answers for any class.
///
/// # Errors
///
/// [`MroError::Cycle`] when `class` reaches itself through its bases, and
/// [`MroError::Inconsistent`] when the bases' orders admit no linearization.
pub fn linearize(
    class: Symbol,
    bases_of: &impl Fn(Symbol) -> Vec<Symbol>,
) -> Result<Vec<Symbol>, MroError> {
    linearize_within(class, bases_of, &mut HashSet::new())
}

/// [`linearize`], with `visiting` holding the classes whose MRO is being
/// computed (a class met again among its own ancestors is a cycle).
fn linearize_within(
    class: Symbol,
    bases_of: &impl Fn(Symbol) -> Vec<Symbol>,
    visiting: &mut HashSet<Symbol>,
) -> Result<Vec<Symbol>, MroError> {
    if !visiting.insert(class) {
        return Err(MroError::Cycle(class));
    }
    let bases = bases_of(class);
    let mut sequences = Vec::with_capacity(bases.len() + 1);
    for base in &bases {
        sequences.push(linearize_within(*base, bases_of, visiting)?);
    }
    visiting.remove(&class);
    sequences.push(bases);
    let mut order = vec![class];
    loop {
        sequences.retain(|sequence| !sequence.is_empty());
        if sequences.is_empty() {
            return Ok(order);
        }
        let head = sequences
            .iter()
            .map(|sequence| sequence[0])
            .find(|candidate| {
                !sequences
                    .iter()
                    .any(|sequence| sequence[1..].contains(candidate))
            });
        let Some(head) = head else {
            return Err(MroError::Inconsistent(
                sequences.iter().map(|sequence| sequence[0]).collect(),
            ));
        };
        order.push(head);
        for sequence in &mut sequences {
            if sequence[0] == head {
                sequence.remove(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A class table of `(class, bases)` rows over symbols `0..`.
    fn table(rows: &[(u32, &[u32])]) -> impl Fn(Symbol) -> Vec<Symbol> + '_ {
        move |class| {
            rows.iter()
                .find(|(name, _)| Symbol(*name) == class)
                .map(|(_, bases)| bases.iter().map(|base| Symbol(*base)).collect())
                .unwrap_or_default()
        }
    }

    /// Symbols for `ids`.
    fn symbols(ids: &[u32]) -> Vec<Symbol> {
        ids.iter().map(|id| Symbol(*id)).collect()
    }

    #[test]
    fn single_inheritance_is_the_base_chain() {
        let bases = table(&[(2, &[1]), (1, &[0])]);
        assert_eq!(linearize(Symbol(2), &bases), Ok(symbols(&[2, 1, 0])));
    }

    #[test]
    fn a_diamond_lists_the_shared_base_once_after_both_sides() {
        // D(B, C), B(A), C(A): D B C A.
        let bases = table(&[(3, &[1, 2]), (1, &[0]), (2, &[0])]);
        assert_eq!(linearize(Symbol(3), &bases), Ok(symbols(&[3, 1, 2, 0])));
    }

    #[test]
    fn the_textbook_example_matches_cpython() {
        // O=0 A=1 B=2 C=3 D=4 E=5 K1=6 K2=7 K3=8 Z=9 (the Python 2.3 MRO paper):
        // K1(A, B, C), K2(D, B, E), K3(D, A), Z(K1, K2, K3)
        // Z K1 K2 K3 D A B C E O
        let bases = table(&[
            (1, &[0]),
            (2, &[0]),
            (3, &[0]),
            (4, &[0]),
            (5, &[0]),
            (6, &[1, 2, 3]),
            (7, &[4, 2, 5]),
            (8, &[4, 1]),
            (9, &[6, 7, 8]),
        ]);
        assert_eq!(
            linearize(Symbol(9), &bases),
            Ok(symbols(&[9, 6, 7, 8, 4, 1, 2, 3, 5, 0]))
        );
    }

    #[test]
    fn conflicting_orders_have_no_linearization() {
        // X(A, B), Y(B, A), Z(X, Y): CPython raises TypeError.
        let bases = table(&[(3, &[1, 2]), (4, &[2, 1]), (5, &[3, 4])]);
        assert!(matches!(
            linearize(Symbol(5), &bases),
            Err(MroError::Inconsistent(_))
        ));
    }

    #[test]
    fn a_cycle_is_reported() {
        let bases = table(&[(1, &[2]), (2, &[1])]);
        assert!(matches!(
            linearize(Symbol(1), &bases),
            Err(MroError::Cycle(_))
        ));
    }
}
