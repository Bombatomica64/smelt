//! Typed Rust protocols for Python operations in Smelt-generated programs.
//!
//! Python spells language protocols as specially named methods such as
//! `__init__` and `__add__`. Generated Rust should not route those statically
//! known operations through a tagged dynamic value. This crate provides small,
//! generic traits that preserve the source signatures until codegen can map a
//! protocol to a standard Rust trait or another concrete implementation.
//!
//! The traits deliberately model Python's borrowing behavior. Calling
//! `__init__` mutates an already allocated instance, while calling `__add__`
//! does not consume either operand. Generated storage types may additionally
//! implement [`std::ops::Add`] when their ownership model makes that idiomatic;
//! [`PyAdd`] remains the precise shared protocol used by Python lowering.

#![forbid(unsafe_code)]

/// Python's instance initializer protocol (`__init__`).
///
/// `Args` is normally a tuple, including the one-element tuple `(T,)`. Using a
/// generic argument pack keeps every source parameter concretely typed without
/// requiring arity-specific traits or erased runtime values.
///
/// Construction is intentionally separate from initialization: Python first
/// allocates an instance through `__new__`, then invokes `__init__` on it.
pub trait PyInit<Args> {
    /// Initialize an allocated instance from a typed argument pack.
    fn py_init(&mut self, args: Args);
}

/// Construct a default-allocatable Python value through its typed initializer.
///
/// This extension trait is a convenience for the common generated-class case.
/// Classes with custom `__new__` semantics can allocate themselves separately
/// and call [`PyInit::py_init`] without using this adapter.
pub trait PyConstruct<Args>: PyInit<Args> + Default + Sized {
    /// Allocate `Self`, run its Python initializer, and return the value.
    fn py_construct(args: Args) -> Self {
        let mut value = Self::default();
        value.py_init(args);
        value
    }
}

impl<T, Args> PyConstruct<Args> for T where T: PyInit<Args> + Default {}

/// Python's forward addition protocol (`__add__`).
///
/// `Rhs` and [`Self::Output`] remain independent so heterogeneous operations
/// such as `Vector + Scalar -> Vector` do not require dynamic dispatch or type
/// erasure. Implementations borrow both operands, matching Python object
/// semantics even when the generated type is not cheaply copyable.
pub trait PyAdd<Rhs = Self> {
    /// Concrete result produced by the Python addition protocol.
    type Output;

    /// Evaluate `self.__add__(rhs)` without consuming either operand.
    fn py_add(&self, rhs: &Rhs) -> Self::Output;
}

impl<T, Rhs> PyAdd<Rhs> for T
where
    T: Clone + std::ops::Add<Rhs>,
    Rhs: Clone,
{
    type Output = <T as std::ops::Add<Rhs>>::Output;

    fn py_add(&self, rhs: &Rhs) -> Self::Output {
        self.clone() + rhs.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{PyAdd, PyConstruct, PyInit};

    /// Small generated-class analogue used to verify typed initialization.
    #[derive(Clone, Default, Debug, PartialEq)]
    struct Point {
        x: i64,
        y: i64,
    }

    impl PyInit<(i64, i64)> for Point {
        fn py_init(&mut self, (x, y): (i64, i64)) {
            self.x = x;
            self.y = y;
        }
    }

    impl std::ops::Add<Point> for Point {
        type Output = Point;

        fn add(self, rhs: Point) -> Self::Output {
            Point {
                x: self.x + rhs.x,
                y: self.y + rhs.y,
            }
        }
    }

    /// Typed initializer arguments survive construction without erasure.
    #[test]
    fn construct_runs_init_with_a_concrete_tuple() {
        let point = Point::py_construct((2, 3));
        assert_eq!(point, Point { x: 2, y: 3 });
    }

    /// Addition borrows its operands and returns the declared concrete output.
    #[test]
    fn add_preserves_operands_and_output_type() {
        let left = Point::py_construct((1, 2));
        let right = Point::py_construct((3, 4));

        let sum: Point = left.py_add(&right);

        assert_eq!(sum, Point { x: 4, y: 6 });
        assert_eq!(left, Point { x: 1, y: 2 });
        assert_eq!(right, Point { x: 3, y: 4 });
    }
}
