//! Built-in showcase examples.
//!
//! Each [`ShowcaseExample`] is a self-contained TypeScript/Python pair that
//! lowers cleanly through the Smelt pipeline today. The set is intentionally
//! conservative — every example is asserted to compile by a test in
//! [`crate::compiler`] — so the gallery only ever advertises working code.

/// What a showcase example is meant to demonstrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShowcaseFocus {
    /// TypeScript and Python sharing one compiled program.
    CrossLanguage,
    /// A typed, library-inspired utility helper.
    LibraryUtility,
    /// Python driving logic that also reaches into shared code.
    PythonInterop,
}

impl ShowcaseFocus {
    /// Short label for the rail badge.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::CrossLanguage => "Cross-language",
            Self::LibraryUtility => "Utility",
            Self::PythonInterop => "Python driver",
        }
    }
}

/// A built-in, ready-to-compile demonstration.
#[derive(Debug, Clone, Copy)]
pub(super) struct ShowcaseExample {
    /// Stable identifier used for selection state.
    pub(super) id: &'static str,
    /// Display title shown in the example rail.
    pub(super) title: &'static str,
    /// One-line purpose statement.
    pub(super) description: &'static str,
    /// Short capability tags.
    pub(super) tags: &'static [&'static str],
    /// TypeScript source (may be empty).
    pub(super) ts_source: &'static str,
    /// Python source (may be empty).
    pub(super) py_source: &'static str,
    /// What the example primarily demonstrates.
    pub(super) focus: ShowcaseFocus,
}

impl ShowcaseExample {
    /// Whether this example ships TypeScript source.
    pub(super) fn has_ts(&self) -> bool {
        !self.ts_source.trim().is_empty()
    }

    /// Whether this example ships Python source.
    pub(super) fn has_py(&self) -> bool {
        !self.py_source.trim().is_empty()
    }

    /// Human-readable source-language summary.
    pub(super) fn languages_label(&self) -> &'static str {
        match (self.has_ts(), self.has_py()) {
            (true, true) => "TypeScript + Python",
            (true, false) => "TypeScript",
            (false, true) => "Python",
            (false, false) => "—",
        }
    }
}

/// The default cross-language sample, used as the initial and reset state.
pub(super) const DEFAULT_EXAMPLE_ID: &str = "cross-language-add";

/// All working showcase examples, shown in the left rail.
pub(super) const EXAMPLES: &[ShowcaseExample] = &[
    ShowcaseExample {
        id: "cross-language-add",
        title: "Cross-language add",
        description: "Python imports and calls a function exported from TypeScript.",
        tags: &["export", "import", "interop"],
        ts_source: "\
export function add(a: number, b: number): number {
    return a + b;
}
",
        py_source: "\
from lib import add

result: float = add(2.0, 3.0)
print(result)
",
        focus: ShowcaseFocus::CrossLanguage,
    },
    ShowcaseExample {
        id: "number-utilities",
        title: "Number utilities",
        description: "A small typed numeric helper library, in the spirit of Remeda.",
        tags: &["pure functions", "numbers"],
        ts_source: "\
export function square(x: number): number {
    return x * x;
}

export function cube(x: number): number {
    return x * x * x;
}

export function average(a: number, b: number): number {
    return (a + b) / 2;
}

console.log(square(8));
console.log(cube(3));
console.log(average(10, 20));
",
        py_source: "",
        focus: ShowcaseFocus::LibraryUtility,
    },
    ShowcaseExample {
        id: "compose-areas",
        title: "Compose across languages",
        description: "Compute a rectangle's area in TypeScript, call it from Python.",
        tags: &["reuse", "interop"],
        ts_source: "\
export function area(width: number, height: number): number {
    return width * height;
}
",
        py_source: "\
from lib import area

space: float = area(4.0, 5.0)
print(space)
",
        focus: ShowcaseFocus::CrossLanguage,
    },
    ShowcaseExample {
        id: "python-driver",
        title: "Python driver",
        description: "Python defines and drives its own typed helper function.",
        tags: &["functions", "locals"],
        ts_source: "",
        py_source: "\
def grow(amount: float, rate: float) -> float:
    return amount + amount * rate

base: float = 1000.0
year1: float = grow(base, 0.05)
year2: float = grow(year1, 0.05)
print(year1)
print(year2)
",
        focus: ShowcaseFocus::PythonInterop,
    },
];

/// Looks up an example by id, falling back to the first example.
pub(super) fn by_id(id: &str) -> &'static ShowcaseExample {
    EXAMPLES
        .iter()
        .find(|example| example.id == id)
        .unwrap_or(&EXAMPLES[0])
}
