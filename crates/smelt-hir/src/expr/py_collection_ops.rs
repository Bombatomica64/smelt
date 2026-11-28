//! Operations on Python's `collections` containers (`deque`, `Counter`,
//! `defaultdict`, `OrderedDict`).
//!
//! Each container is a concrete generic runtime type (`smelt-runtime`'s
//! `py_collections.rs`), so an operation on one is selected statically, as a
//! `HeadersOp` is: the frontend proves the receiver's class and the operand
//! types, and the backend writes one method call. A variant names the RUST
//! method and its calling convention, not a Python spelling: `items` on a
//! deque (its elements) and on a mapping (its pairs) are one variant, because
//! the call text is the same and the frontend gives each its own result type.

use serde::{Deserialize, Serialize};

/// Which container a `PyCollectionNew` builds, and from what.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PyCollectionCtor {
    /// `deque(items, maxlen)`: `[items: list[T], maxlen: Optional[machine int]]`;
    /// a negative bound raises `ValueError`.
    Deque,
    /// `Counter(items)`: `[items: list[K]]`, each item counted once.
    Counter,
    /// `Counter(mapping)`: `[pairs: list[tuple[K, int]]]`.
    CounterFromCounts,
    /// `defaultdict(factory, pairs)`: `[factory: () -> V, factory_repr: str,
    /// pairs: list[tuple[K, V]]]`.
    DefaultDict,
    /// `OrderedDict(pairs)`: `[pairs: list[tuple[K, V]]]`.
    OrderedDict,
}

impl PyCollectionCtor {
    /// The runtime type's constructor, as `<Type>::{name}`.
    #[must_use]
    pub const fn rust_name(self) -> &'static str {
        match self {
            Self::Deque | Self::Counter | Self::DefaultDict | Self::OrderedDict => "new",
            Self::CounterFromCounts => "from_counts",
        }
    }

    /// Whether building the container can raise (only a negative deque bound).
    #[must_use]
    pub const fn fallible(self) -> bool {
        matches!(self, Self::Deque)
    }

    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Deque => "deque",
            Self::Counter => "counter",
            Self::CounterFromCounts => "counter_from_counts",
            Self::DefaultDict => "defaultdict",
            Self::OrderedDict => "ordered_dict",
        }
    }
}

impl std::fmt::Display for PyCollectionCtor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.text())
    }
}

/// One method of a `collections` container's runtime type.
///
/// The receiver is the op's `receiver` operand; the arguments follow in the
/// order the variant documents. A machine-integer argument is an `i64`
/// position or count the frontend already converted from a Python `int`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PyCollectionMethod {
    /// `d.append(x)`: `[item]`.
    Append,
    /// `d.appendleft(x)`: `[item]`.
    AppendLeft,
    /// `d.pop()`: `[]`, `IndexError` when empty.
    DequePop,
    /// `d.popleft()`: `[]`, `IndexError` when empty.
    PopLeft,
    /// `d.extend(xs)`: `[list]`.
    Extend,
    /// `d.extendleft(xs)`: `[list]`.
    ExtendLeft,
    /// `d.rotate(n)`: `[machine int]`.
    Rotate,
    /// `x.clear()`: `[]`.
    Clear,
    /// `len(x)`: `[]`, a machine integer.
    Len,
    /// `d[i]`: `[machine int]`, `IndexError` out of range.
    GetIndex,
    /// `d[i] = x`: `[machine int, item]`, `IndexError` out of range.
    SetIndex,
    /// `x in c`: `[item]`, read by reference.
    Contains,
    /// `d.count(x)`: `[item]`, read by reference.
    Count,
    /// `d.index(x)`: `[item]`, `ValueError` when absent.
    Index,
    /// `d.remove(x)`: `[item]`, `ValueError` when absent.
    Remove,
    /// `d.reverse()`: `[]`.
    Reverse,
    /// `x.copy()`: `[]`, a new container.
    Copy,
    /// `d.maxlen`: `[]`, an optional `int`.
    MaxLen,
    /// A deque's elements, or a mapping's `items()` pairs: `[]`, a new list.
    Items,
    /// A mapping's or counter's keys in order: `[]`, a new list.
    Keys,
    /// A mapping's or counter's values in order: `[]`, a new list.
    Values,
    /// `x == y` of two containers of one class: `[other]`, by reference.
    Equals,
    /// `c[k]` (a missing key counts `0`) or `d.get(k)` (an optional value):
    /// `[key]`, by reference; never inserts.
    Get,
    /// `d.get(k, default)`: `[key, default]`, the key by reference.
    GetOr,
    /// `x[k] = v`: `[key, value]`.
    Set,
    /// `del c[k]` on a `Counter`: `[key]`, a missing key is no error.
    CounterDelete,
    /// `del d[k]` on a mapping: `[key]`, `KeyError` when missing.
    MappingDelete,
    /// `d.pop(k)`: `[key]`, `KeyError` when missing.
    MappingPop,
    /// `d.pop(k, default)`: `[key, default]`, the key by reference.
    PopOr,
    /// `d.setdefault(k, v)`: `[key, value]`.
    SetDefault,
    /// `c.update(xs)`: `[list]`, one more per item.
    Update,
    /// `c.update(other_counter)`: `[counter]`, by reference.
    UpdateCounts,
    /// `c.subtract(xs)`: `[list]`.
    Subtract,
    /// `c.subtract(other_counter)`: `[counter]`, by reference.
    SubtractCounts,
    /// `c.most_common(n)`: `[optional machine int]`.
    MostCommon,
    /// `c.elements()`: `[]`, a new list.
    Elements,
    /// `c.total()`: `[]`.
    Total,
    /// `c1 + c2`: `[counter]`, by reference; a new counter.
    Plus,
    /// `c1 - c2`: `[counter]`, by reference; a new counter.
    Minus,
    /// `d[k]` on a `defaultdict`: `[key]`, inserting the factory's value for
    /// a missing key (which runs the factory).
    GetOrInsert,
    /// `od[k]` on an `OrderedDict`: `[key]`, `KeyError` when missing.
    GetItem,
    /// `od.popitem(last)`: `[bool]`, `KeyError` when empty.
    PopItem,
    /// `od.move_to_end(k, last)`: `[key, bool]`, `KeyError` when missing.
    MoveToEnd,
}

impl PyCollectionMethod {
    /// The runtime type's method this variant calls.
    #[must_use]
    pub const fn rust_name(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::AppendLeft => "appendleft",
            Self::DequePop | Self::MappingPop => "pop",
            Self::PopLeft => "popleft",
            Self::Extend => "extend",
            Self::ExtendLeft => "extendleft",
            Self::Rotate => "rotate",
            Self::Clear => "clear",
            Self::Len => "len",
            Self::GetIndex => "get",
            Self::SetIndex | Self::Set => "set",
            Self::Contains => "contains",
            Self::Count => "count",
            Self::Index => "index",
            Self::Remove => "remove",
            Self::Reverse => "reverse",
            Self::Copy => "copy",
            Self::MaxLen => "maxlen",
            Self::Items => "items",
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Equals => "equals",
            Self::Get => "get",
            Self::GetOr => "get_or",
            Self::CounterDelete | Self::MappingDelete => "delete",
            Self::PopOr => "pop_or",
            Self::SetDefault => "setdefault",
            Self::Update => "update",
            Self::UpdateCounts => "update_counts",
            Self::Subtract => "subtract",
            Self::SubtractCounts => "subtract_counts",
            Self::MostCommon => "most_common",
            Self::Elements => "elements",
            Self::Total => "total",
            Self::Plus => "plus",
            Self::Minus => "minus",
            Self::GetOrInsert => "get_or_insert",
            Self::GetItem => "get_item",
            Self::PopItem => "popitem",
            Self::MoveToEnd => "move_to_end",
        }
    }

    /// Whether the call can raise a Python exception (`IndexError`,
    /// `ValueError`, `KeyError`).
    #[must_use]
    pub const fn fallible(self) -> bool {
        matches!(
            self,
            Self::DequePop
                | Self::PopLeft
                | Self::GetIndex
                | Self::SetIndex
                | Self::Index
                | Self::Remove
                | Self::MappingDelete
                | Self::MappingPop
                | Self::GetItem
                | Self::PopItem
                | Self::MoveToEnd
        )
    }

    /// Whether the call passes its first argument by reference (a key or
    /// item the method only reads, or another container).
    #[must_use]
    pub const fn first_arg_by_ref(self) -> bool {
        matches!(
            self,
            Self::Contains
                | Self::Count
                | Self::Index
                | Self::Remove
                | Self::Equals
                | Self::Get
                | Self::GetOr
                | Self::CounterDelete
                | Self::MappingDelete
                | Self::MappingPop
                | Self::PopOr
                | Self::UpdateCounts
                | Self::SubtractCounts
                | Self::Plus
                | Self::Minus
                | Self::GetItem
                | Self::MoveToEnd
        )
    }

    /// Whether the call changes the receiver's contents (behind its shared
    /// handle), or runs source code (a `defaultdict` factory).
    #[must_use]
    pub const fn has_effect(self) -> bool {
        !matches!(
            self,
            Self::Len
                | Self::GetIndex
                | Self::Contains
                | Self::Count
                | Self::Index
                | Self::Copy
                | Self::MaxLen
                | Self::Items
                | Self::Keys
                | Self::Values
                | Self::Equals
                | Self::Get
                | Self::GetOr
                | Self::MostCommon
                | Self::Elements
                | Self::Total
                | Self::Plus
                | Self::Minus
                | Self::GetItem
        )
    }
}

impl std::fmt::Display for PyCollectionMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.rust_name())
    }
}
