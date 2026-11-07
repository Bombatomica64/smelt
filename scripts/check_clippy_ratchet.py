#!/usr/bin/env python3
"""Judge one `cargo clippy --message-format=json` run against a baseline.

Two rules, both blocking:

* Every deny-level lint fails: any `error` diagnostic, or a nonzero cargo exit.
* Warn-level lints may only fall: the count of each lint in each crate may not
  rise above `blocker-logs/clippy-baseline.json`. A lint new to a crate has a
  baseline of zero.

`-D warnings` is not used because the warn-level backlog includes the text of
`smelt-runtime`, which is emitted verbatim into every generated crate, so
clearing it there is a change to generated output and not a lint fix.

A run that compiled nothing fails, which is how a missing clippy component shows
up. Run with `--update` to write the falls into the baseline. It can never take a
rise. A new baseline is only written when the file does not exist.
"""

import argparse
import json
import os
import sys
from collections import Counter

MINIMUM_ARTIFACTS = 1


def read_messages(path):
    """Yield the JSON objects of a cargo message stream, skipping other lines."""
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if line.startswith("{"):
                yield json.loads(line)


def collect(path):
    """Return (per-crate lint counts, rendered errors, compiled artifact count).

    A diagnostic reached from two targets of a crate (lib and lib test) is one
    diagnostic, so sites are deduplicated by file, line, column and lint.
    """
    sites = set()
    errors = {}
    artifacts = 0
    for message in read_messages(path):
        if message.get("reason") == "compiler-artifact":
            artifacts += 1
        if message.get("reason") != "compiler-message":
            continue
        diagnostic = message["message"]
        code = (diagnostic.get("code") or {}).get("code")
        spans = [span for span in diagnostic.get("spans", []) if span.get("is_primary")]
        if diagnostic["level"].startswith("error"):
            errors[diagnostic.get("rendered") or diagnostic["message"]] = True
        elif code and spans:
            crate = os.path.basename(os.path.dirname(message["manifest_path"]))
            span = spans[0]
            sites.add((crate, code, span["file_name"], span["line_start"], span["column_start"]))
    counts = Counter((crate, code) for crate, code, _, _, _ in sites)
    nested = {}
    for (crate, code), count in sorted(counts.items()):
        nested.setdefault(crate, {})[code] = count
    return nested, list(errors), artifacts


def rises(counts, baseline):
    """Return one line per (crate, lint) above its baseline count."""
    lines = []
    for crate, lints in sorted(counts.items()):
        allowed = baseline.get(crate, {})
        for lint, count in sorted(lints.items()):
            limit = allowed.get(lint, 0)
            if count > limit:
                lines.append(f"{crate}: {lint}: {limit} -> {count}")
    return lines


def lowered(counts, baseline):
    """The baseline with every fall taken and every rise refused."""
    updated = {}
    for crate, lints in baseline.items():
        kept = {}
        for lint, limit in lints.items():
            count = min(limit, counts.get(crate, {}).get(lint, 0))
            if count:
                kept[lint] = count
        if kept:
            updated[crate] = kept
    return updated


def write_baseline(path, counts):
    """Write the baseline file."""
    document = {
        "_comment": "Warn-level clippy lints per crate; see scripts/check_clippy_ratchet.py. "
                    "A count may only fall.",
        "counts": counts,
    }
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=2, sort_keys=True)
        handle.write("\n")


def main(argv=None):
    """CLI entry point; returns the process exit status."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--messages", required=True, help="cargo JSON message stream")
    parser.add_argument("--cargo-status", type=int, required=True, help="cargo's exit status")
    parser.add_argument("--update", action="store_true", help="write the falls to the baseline")
    args = parser.parse_args(argv)

    counts, errors, artifacts = collect(args.messages)
    if artifacts < MINIMUM_ARTIFACTS:
        print(f"clippy compiled nothing (cargo exit {args.cargo_status}); it did not run.")
        return 1
    for rendered in errors:
        print(rendered)
    if errors or args.cargo_status != 0:
        print(f"{len(errors)} deny-level diagnostic(s); cargo exit {args.cargo_status}.")
        return 1

    total = sum(sum(lints.values()) for lints in counts.values())
    if not os.path.exists(args.baseline):
        if args.update:
            write_baseline(args.baseline, counts)
            print(f"Wrote {args.baseline}: {total} warning(s).")
            return 0
        print(f"baseline {args.baseline} does not exist")
        return 1
    with open(args.baseline, encoding="utf-8") as handle:
        baseline = json.load(handle)["counts"]

    worse = rises(counts, baseline)
    if args.update:
        write_baseline(args.baseline, lowered(counts, baseline))
    print(f"Clippy: 0 deny-level, {total} warn-level "
          f"(baseline {sum(sum(lints.values()) for lints in baseline.values())}).")
    for line in worse:
        print(f"- {line}")
    if worse:
        print("Clippy ratchet: FAIL. Fix the new warnings; the baseline cannot take a rise.")
        return 1
    print("Clippy ratchet: PASS.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
