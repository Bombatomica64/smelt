//! The textual scan: walking generated `.rs` files, counting `SmeltUnknown`
//! occurrences per category and grouping them by normalized line shape.

use super::classify::{AdapterBody, LineSite, PreludeBoundary, classify_line};
use super::{Category, ShapeGroup, UnknownReport, display_path};
use crate::{CliResult, report::{SchemaVersion, bump}};
use std::{collections::BTreeMap, path::PathBuf};

/// Identity of one shape group: its category and normalized line pattern.
type ShapeKey = (Category, String);

/// Result of scanning a corpus: the serializable report plus a per-file
/// avoidable-erasure tally used to name top offenders in a regression message.
///
/// The per-file tally is deliberately kept out of [`UnknownReport`] so the
/// checked-in baseline JSON stays small and stable across large corpora (a
/// per-file map would add one entry per generated file).
pub(super) struct ScanResult {
    /// The serializable, checked-in report.
    pub(super) report: UnknownReport,
    /// Avoidable-erasure occurrences keyed by displayed file path.
    pub(super) avoidable_by_file: BTreeMap<String, usize>,
}

/// Scan every file, tallying `SmeltUnknown` occurrences per shape and category.
pub(super) fn scan_files(files: &[PathBuf]) -> CliResult<ScanResult> {
    let mut occurrences: BTreeMap<ShapeKey, usize> = BTreeMap::new();
    let mut file_sets: BTreeMap<ShapeKey, usize> = BTreeMap::new();
    let mut examples: BTreeMap<ShapeKey, String> = BTreeMap::new();
    let mut category_totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut avoidable_by_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;

    // Resolve the current directory once so `file:line` examples are recorded
    // relative to it, keeping committed baselines portable across machines.
    let cwd = std::env::current_dir().ok();
    for file in files {
        let text = std::fs::read_to_string(file)
            .map_err(|error| format!("failed to read `{}`: {error}", file.display()))?;
        let display = display_path(file, cwd.as_deref());
        // Track which shapes appeared in this file to count distinct files.
        let mut seen_here: std::collections::BTreeSet<ShapeKey> = std::collections::BTreeSet::new();
        let mut boundary = PreludeBoundary::of(&text);
        let mut adapter = AdapterBody::default();
        for (index, line) in text.lines().enumerate() {
            let site = boundary.advance(index, line);
            let in_adapter = adapter.advance(line);
            let count = count_value_erasures(line);
            if count == 0 {
                continue;
            }
            let category = match site {
                LineSite::Prelude => Category::RuntimePrelude,
                LineSite::Program { in_prelude_helper: false } if in_adapter => {
                    Category::LegitimateBoundary
                }
                LineSite::Program { in_prelude_helper } => classify_line(line, in_prelude_helper),
            };
            let shape = normalize_line(line);
            let key = (category, shape.clone());
            bump(&mut occurrences, key.clone(), count);
            bump(&mut category_totals, category.as_str().to_owned(), count);
            if category == Category::AvoidableErasure {
                bump(&mut avoidable_by_file, display.clone(), count);
            }
            total = total.saturating_add(count);
            examples
                .entry(key.clone())
                .or_insert_with(|| format!("{display}:{}", index.saturating_add(1)));
            seen_here.insert(key);
        }
        for key in seen_here {
            bump(&mut file_sets, key, 1);
        }
    }

    let mut shapes: Vec<ShapeGroup> = occurrences
        .into_iter()
        .map(|((category, shape), count)| {
            let key = (category, shape.clone());
            ShapeGroup {
                category,
                shape,
                occurrences: count,
                files: file_sets.get(&key).copied().unwrap_or(0),
                example: examples.get(&key).cloned().unwrap_or_default(),
            }
        })
        .collect();
    // Most frequent first; ties broken by category then shape for stability.
    shapes.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then(left.category.cmp(&right.category))
            .then(left.shape.cmp(&right.shape))
    });

    Ok(ScanResult {
        report: UnknownReport {
            version: SchemaVersion::CURRENT,
            files_scanned: files.len(),
            total,
            category_totals,
            shapes,
            erasure: None,
        },
        avoidable_by_file,
    })
}

/// Count non-overlapping occurrences of `needle` in `haystack`.
pub(super) fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    let mut rest = haystack;
    while let Some(index) = rest.find(needle) {
        count = count.saturating_add(1);
        rest = &rest[index.saturating_add(needle.len())..];
    }
    count
}

/// Count the `SmeltUnknown` occurrences on a line that represent *erased values*,
/// excluding pure type-annotation spellings that carry no additional erasure
/// signal.
///
/// # Why annotation tokens are excluded
///
/// The metric measures how many *values* fall back to the erased tagged union,
/// not how many times the `SmeltUnknown` identifier is typed. When PR #156 made
/// generated functions fallible, two purely syntactic changes inflated the raw
/// token count without erasing a single additional value:
///
/// - Return sites gained a `Ok::<SmeltUnknown, Box<dyn std::error::Error>>(expr)`
///   turbofish where a bare `expr` stood before — one extra `SmeltUnknown` token
///   per fallible return that annotates the `Result` `Ok` arm, not a new value.
/// - Some erased closures gained an explicit typed signature
///   (`... -> Result<SmeltUnknown, Box<dyn std::error::Error>>`) where an already
///   erased `SmeltErasedFunction` literal stood before; the `SmeltUnknown` inside
///   that return annotation is type spelling, not a distinct erased value.
///
/// Wrapping a function in `Result` for fallibility must be **count-neutral**: the
/// erased value it returns is already counted at its construction site, so these
/// annotation spellings are stripped before counting. The strip is deterministic
/// (exact spellings only) so the tally stays stable and diffable across runs.
pub(super) fn count_value_erasures(line: &str) -> usize {
    let normalized = strip_annotation_spellings(line);
    count_occurrences(&normalized, "SmeltUnknown")
}

/// Strip fallibility-wrapping type-annotation spellings from a line so their
/// `SmeltUnknown` tokens are not counted as erased values. See
/// [`count_value_erasures`] for the rationale.
///
/// Only exact, unambiguous spellings are rewritten so the transform is
/// deterministic and never accidentally hides a genuine erased value:
/// - the `Ok::<SmeltUnknown, Box<dyn std::error::Error>>` return turbofish loses
///   its annotation entirely (`Ok`);
/// - the `-> Result<SmeltUnknown, Box<dyn std::error::Error>>` return annotation
///   has its `SmeltUnknown` replaced with `()` so the surrounding shape is
///   preserved while the token no longer counts.
fn strip_annotation_spellings(line: &str) -> String {
    line.replace("Ok::<SmeltUnknown, Box<dyn std::error::Error>>", "Ok")
        .replace(
            "-> Result<SmeltUnknown, Box<dyn std::error::Error>>",
            "-> Result<(), Box<dyn std::error::Error>>",
        )
}

/// Normalize a line into a stable shape so structurally identical call sites
/// collapse onto one group.
///
/// Erased per-site specifics:
/// - string/char literals become `"S"` / `'C'`;
/// - runs of ASCII digits become `N`;
/// - leading indentation is stripped and internal whitespace runs collapse to a
///   single space, so formatting differences never split a group.
///
/// Identifiers are intentionally *kept*: the surrounding identifiers (e.g.
/// `Vec<SmeltUnknown>`, `SmeltUnknown::Number`) are the informative part of the
/// shape, so unlike the probe normalizer this does not blank them out.
pub(super) fn normalize_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.trim().chars().peekable();
    let mut last_was_space = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                out.push_str("\"S\"");
                // Skip the string body, honoring escapes.
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
                last_was_space = false;
            }
            '\'' => {
                out.push_str("'C'");
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => {
                            chars.next();
                        }
                        '\'' => break,
                        _ => {}
                    }
                }
                last_was_space = false;
            }
            digit if digit.is_ascii_digit() => {
                out.push('N');
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                }
                last_was_space = false;
            }
            space if space.is_whitespace() => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            other => {
                out.push(other);
                last_was_space = false;
            }
        }
    }
    out.trim().to_owned()
}
