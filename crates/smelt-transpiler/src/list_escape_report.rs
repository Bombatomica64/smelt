//! Markdown/JSON reporting for the MIR list escape analysis.
//!
//! Lowers a Smelt manifest all the way to optimized MIR — the same MIR codegen
//! consumes — runs [`smelt_mir::analyze_list_escapes`], and renders the
//! population counts. The point of the command is to price a hypothetical
//! "tiered" list representation (a plain `Vec<T>` for lists that provably need
//! neither a shared buffer nor interior mutability) *before* building it: the
//! report says how many list-typed locals would qualify and where they are.
//!
//! The command changes nothing about codegen. It exists so the decision is made
//! from a count rather than from inspection of a few hand-picked functions.
//!
//! Escape reasons are split into genuine escapes (returned, passed, captured,
//! stored, erased) and conservative ones (the analysis could not prove
//! confinement). That split matters when reading the numbers: only the
//! conservative half could ever move with a more precise analysis.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

use smelt_mir::{BodyKey, BodyListEscape, EscapeReason, ListLocalClass};

use crate::{lowering, pipeline};

/// Output format for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListEscapeReportFormat {
    /// Human-readable Markdown tables.
    Markdown,
    /// Machine-readable JSON, suitable as a checked-in baseline.
    Json,
}

/// Inputs for one report run.
pub(crate) struct ListEscapeReportOptions<'a> {
    /// Smelt manifest to lower.
    pub manifest: &'a Path,
    /// Rendering format.
    pub format: ListEscapeReportFormat,
    /// Function names to expand into a per-local detail section. Matching is on
    /// the MIR function name, which is the snake_cased source name.
    pub functions: &'a [String],
    /// How many rows the "top bodies by local lists" table keeps.
    pub top: usize,
}

/// Tallies for one corpus or one body group.
#[derive(Debug, Default, Clone, Copy)]
struct ClassCounts {
    /// Lists whose buffer can be observed outside the frame.
    escaping: usize,
    /// Lists confined to the frame but named by more than one live local.
    aliased: usize,
    /// Confined, single-named, never written through.
    local_immutable: usize,
    /// Confined, single-named, written through in place.
    local_mutated: usize,
}

impl ClassCounts {
    /// Total list-typed locals counted.
    const fn total(self) -> usize {
        self.escaping + self.aliased + self.local_immutable + self.local_mutated
    }

    /// Confined lists — the population a tiered representation could take.
    const fn confined(self) -> usize {
        self.local_immutable + self.local_mutated
    }

    /// Add one classified local.
    const fn add(&mut self, class: ListLocalClass) {
        match class {
            ListLocalClass::Escaping => self.escaping += 1,
            ListLocalClass::Aliased => self.aliased += 1,
            ListLocalClass::LocalImmutable => self.local_immutable += 1,
            ListLocalClass::LocalMutated => self.local_mutated += 1,
        }
    }
}

/// Render `count` as `n (p.p%)` against `total`, tolerating a zero total.
fn share(count: usize, total: usize) -> String {
    if total == 0 {
        return format!("{count} (—)");
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts here are report percentages, far below f64's exact-integer range"
    )]
    let percent = (count as f64) * 100.0 / (total as f64);
    format!("{count} ({percent:.1}%)")
}

/// Lower `manifest` to optimized MIR and render the list-escape report.
pub(crate) fn list_escape_report(
    options: &ListEscapeReportOptions<'_>,
) -> Result<String, Box<dyn std::error::Error>> {
    let manifest_text = options
        .manifest
        .to_str()
        .ok_or("manifest path contains invalid UTF-8")?;
    let config = crate::config_parser::parse(manifest_text)?;
    let (krate, _modules) = lowering::lower_manifest_entries(&config, options.manifest)?;
    let mir = pipeline::lower_to_optimized_mir(&krate)?;
    let bodies = smelt_mir::analyze_list_escapes(&mir);

    Ok(match options.format {
        ListEscapeReportFormat::Markdown => {
            render_markdown(config.project_name(), &bodies, options)
        }
        ListEscapeReportFormat::Json => render_json(config.project_name(), &bodies)?,
    })
}

/// Sum the class counts of every body matching `keep`.
fn tally(bodies: &[BodyListEscape], keep: impl Fn(BodyKey) -> bool) -> ClassCounts {
    tally_bodies(bodies, |body| keep(body.key))
}

/// Sum the class counts of every body matching a whole-body predicate.
fn tally_bodies(
    bodies: &[BodyListEscape],
    keep: impl Fn(&BodyListEscape) -> bool,
) -> ClassCounts {
    let mut counts = ClassCounts::default();
    for body in bodies.iter().filter(|body| keep(body)) {
        for fact in &body.locals {
            counts.add(fact.class);
        }
    }
    counts
}

/// Sum the class counts once per *buffer*, not once per local.
///
/// HIR lowering routes one source array through a chain of moved temporaries,
/// so counting locals over-counts allocation sites by the chain length. Every
/// local in a buffer group shares a class, so collapsing to one row per
/// `(body, group)` gives the number of `Rc<RefCell<Vec<T>>>` allocations the
/// program actually performs.
fn tally_buffers(
    bodies: &[BodyListEscape],
    keep: impl Fn(&BodyListEscape) -> bool,
) -> ClassCounts {
    let mut counts = ClassCounts::default();
    for body in bodies.iter().filter(|body| keep(body)) {
        let mut seen = BTreeSet::new();
        for fact in &body.locals {
            if seen.insert(fact.group.0) {
                counts.add(fact.class);
            }
        }
    }
    counts
}

/// Render the Markdown report.
fn render_markdown(
    project: &str,
    bodies: &[BodyListEscape],
    options: &ListEscapeReportOptions<'_>,
) -> String {
    let mut out = String::new();
    let all = tally(bodies, |_| true);
    let functions = tally(bodies, |key| matches!(key, BodyKey::Function(_)));
    let closures = tally(bodies, |key| matches!(key, BodyKey::Closure(_)));
    let product = tally_bodies(bodies, |body| !body.is_test);
    let tests = tally_bodies(bodies, |body| body.is_test);
    let buffers = tally_buffers(bodies, |_| true);
    let product_buffers = tally_buffers(bodies, |body| !body.is_test);

    let _ = writeln!(out, "# List escape report — `{project}`\n");
    let _ = writeln!(
        out,
        "Every list-typed MIR local in every function and closure body, classified by \
         whether a plain `Vec<T>` could replace its `Rc<RefCell<Vec<T>>>`. Unprovable \
         cases are counted as `escaping`; see the `smelt_mir::list_escape` module \
         docs for exactly which constructs that covers.\n"
    );

    let _ = writeln!(out, "## Population\n");
    let _ = writeln!(
        out,
        "| class | all bodies | functions | closures |\n|---|---:|---:|---:|"
    );
    for (label, pick) in [
        (
            "escaping",
            (all.escaping, functions.escaping, closures.escaping),
        ),
        ("aliased", (all.aliased, functions.aliased, closures.aliased)),
        (
            "local-mutated",
            (
                all.local_mutated,
                functions.local_mutated,
                closures.local_mutated,
            ),
        ),
        (
            "local-immutable",
            (
                all.local_immutable,
                functions.local_immutable,
                closures.local_immutable,
            ),
        ),
    ] {
        let _ = writeln!(
            out,
            "| {label} | {} | {} | {} |",
            share(pick.0, all.total()),
            share(pick.1, functions.total()),
            share(pick.2, closures.total()),
        );
    }
    let _ = writeln!(
        out,
        "| **total** | {} | {} | {} |\n",
        all.total(),
        functions.total(),
        closures.total()
    );
    let _ = writeln!(
        out,
        "Confined (`local-*`, the tierable population): **{}** of {} list locals.\n",
        share(all.confined(), all.total()),
        all.total()
    );

    // One buffer can be named by several MIR locals, because lowering moves a
    // value through a chain of temporaries. Allocations track buffers, so this
    // is the number that prices the optimization.
    let _ = writeln!(out, "### Per buffer, not per local\n");
    let _ = writeln!(
        out,
        "One `Rc<RefCell<Vec<T>>>` can be named by several MIR locals (lowering moves a \
         value through a chain of temporaries; every local in a group shares a class). \
         Collapsing each group to one row gives the allocation-site count.\n"
    );
    let _ = writeln!(
        out,
        "| class | all buffers | non-test buffers |\n|---|---:|---:|"
    );
    for (label, everywhere, non_test) in [
        ("escaping", buffers.escaping, product_buffers.escaping),
        ("aliased", buffers.aliased, product_buffers.aliased),
        (
            "local-mutated",
            buffers.local_mutated,
            product_buffers.local_mutated,
        ),
        (
            "local-immutable",
            buffers.local_immutable,
            product_buffers.local_immutable,
        ),
    ] {
        let _ = writeln!(
            out,
            "| {label} | {} | {} |",
            share(everywhere, buffers.total()),
            share(non_test, product_buffers.total()),
        );
    }
    let _ = writeln!(
        out,
        "| **total** | {} | {} |\n",
        buffers.total(),
        product_buffers.total()
    );

    // Test bodies dominate a library corpus by line count, and a confined list
    // inside a `#[test]` is not code any runtime optimization would speed up.
    // Splitting them keeps the headline number honest. Closure bodies are not
    // attributable to a test, so they land in the non-test column.
    let _ = writeln!(out, "## Product code vs test code\n");
    let _ = writeln!(
        out,
        "| class | non-test bodies | `#[test]` bodies |\n|---|---:|---:|"
    );
    for (label, non_test, test) in [
        ("escaping", product.escaping, tests.escaping),
        ("aliased", product.aliased, tests.aliased),
        ("local-mutated", product.local_mutated, tests.local_mutated),
        (
            "local-immutable",
            product.local_immutable,
            tests.local_immutable,
        ),
    ] {
        let _ = writeln!(
            out,
            "| {label} | {} | {} |",
            share(non_test, product.total()),
            share(test, tests.total()),
        );
    }
    let _ = writeln!(
        out,
        "| **total** | {} | {} |\n",
        product.total(),
        tests.total()
    );
    let _ = writeln!(
        out,
        "Confined outside tests: **{}** of {} non-test list locals.\n",
        share(product.confined(), product.total()),
        product.total(),
    );

    let _ = writeln!(out, "## Why the escaping lists escape\n");
    let _ = writeln!(out, "| reason | kind | count |\n|---|---|---:|");
    let mut reasons: BTreeMap<&'static str, (bool, usize)> = BTreeMap::new();
    for body in bodies {
        for reason in body.locals.iter().filter_map(|fact| fact.reason) {
            let entry = reasons.entry(reason.label()).or_insert((
                reason.is_genuine(),
                0,
            ));
            entry.1 += 1;
        }
    }
    let mut rows = reasons.into_iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| right.1.1.cmp(&left.1.1).then(left.0.cmp(right.0)));
    let genuine: usize = rows
        .iter()
        .filter(|(_, (is_genuine, _))| *is_genuine)
        .map(|(_, (_, count))| *count)
        .sum();
    for (label, (is_genuine, count)) in &rows {
        let kind = if *is_genuine {
            "genuine"
        } else {
            "conservative"
        };
        let _ = writeln!(out, "| {label} | {kind} | {count} |");
    }
    let _ = writeln!(
        out,
        "\n{} of {} escapes are genuine; the remaining {} are conservative — a more \
         precise analysis could in principle recover some of them.\n",
        genuine,
        all.escaping,
        all.escaping.saturating_sub(genuine),
    );

    let _ = writeln!(out, "## Top non-test bodies by confined list locals\n");
    let _ = writeln!(
        out,
        "| body | local-mutated | local-immutable | aliased | escaping |\n|---|---:|---:|---:|---:|"
    );
    let mut ranked = bodies
        .iter()
        .filter(|body| body.local_count() > 0)
        .collect::<Vec<_>>();
    ranked.retain(|body| !body.is_test);
    ranked.sort_by(|left, right| {
        right
            .local_count()
            .cmp(&left.local_count())
            .then_with(|| left.name.cmp(&right.name))
    });
    for body in ranked.iter().take(options.top) {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            body.name,
            body.count(ListLocalClass::LocalMutated),
            body.count(ListLocalClass::LocalImmutable),
            body.count(ListLocalClass::Aliased),
            body.count(ListLocalClass::Escaping),
        );
    }
    if ranked.is_empty() {
        let _ = writeln!(out, "| _(none)_ | 0 | 0 | 0 | 0 |");
    }
    out.push('\n');

    if !options.functions.is_empty() {
        let _ = writeln!(out, "## Requested functions\n");
        for wanted in options.functions {
            let matches = bodies
                .iter()
                .filter(|body| body.name == *wanted)
                .collect::<Vec<_>>();
            let _ = writeln!(out, "### `{wanted}`\n");
            if matches.is_empty() {
                let _ = writeln!(out, "_no body with that name, or it has no list locals._\n");
                continue;
            }
            for body in matches {
                let _ = writeln!(out, "| local | name | class | reason | mutated |");
                let _ = writeln!(out, "|---|---|---|---|---|");
                for fact in &body.locals {
                    let _ = writeln!(
                        out,
                        "| `%{}` | {} | {} | {} | {} |",
                        fact.local.0,
                        fact.name.as_deref().unwrap_or("_(temp)_"),
                        fact.class.label(),
                        fact.reason.map_or("—", EscapeReason::label),
                        if fact.mutated { "yes" } else { "no" },
                    );
                }
                out.push('\n');
            }
        }
    }
    out
}

/// Render the JSON form: totals plus one entry per body.
fn render_json(
    project: &str,
    bodies: &[BodyListEscape],
) -> Result<String, Box<dyn std::error::Error>> {
    let all = tally(bodies, |_| true);
    let buffers = tally_buffers(bodies, |_| true);
    let body_entries = bodies
        .iter()
        .map(|body| {
            serde_json::json!({
                "body": body.name,
                "kind": match body.key {
                    BodyKey::Function(_) => "function",
                    BodyKey::Closure(_) => "closure",
                },
                "is_test": body.is_test,
                "locals": body
                    .locals
                    .iter()
                    .map(|fact| serde_json::json!({
                        "local": fact.local.0,
                        "group": fact.group.0,
                        "name": fact.name,
                        "class": fact.class.label(),
                        "reason": fact.reason.map(EscapeReason::label),
                        "mutated": fact.mutated,
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let document = serde_json::json!({
        "project": project,
        "totals": {
            "escaping": all.escaping,
            "aliased": all.aliased,
            "local_mutated": all.local_mutated,
            "local_immutable": all.local_immutable,
            "total": all.total(),
        },
        "buffer_totals": {
            "escaping": buffers.escaping,
            "aliased": buffers.aliased,
            "local_mutated": buffers.local_mutated,
            "local_immutable": buffers.local_immutable,
            "total": buffers.total(),
        },
        "bodies": body_entries,
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}
