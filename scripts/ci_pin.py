#!/usr/bin/env python3
"""Print one pin from `.github/compat/libraries.json`, the only home of pins.

    ci_pin.py tool <name>      a tool version, e.g. `tool node`
    ci_pin.py ref <library>    a library's pinned commit
    ci_pin.py repo <library>   a library's `owner/name`
"""

import json
import os
import sys

CONFIG = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    ".github", "compat", "libraries.json",
)


def pin(config, kind, name):
    """Return the pin `kind` of `name` in `config`; a missing one is a KeyError."""
    if kind == "tool":
        return config["tools"][name]
    for library in config["libraries"]:
        if library["name"] == name:
            return library[kind]
    raise KeyError(name)


def main(argv):
    """CLI entry point; returns the process exit status."""
    if len(argv) != 2 or argv[0] not in ("tool", "ref", "repo"):
        print(__doc__, file=sys.stderr)
        return 2
    with open(CONFIG, encoding="utf-8") as handle:
        config = json.load(handle)
    try:
        print(pin(config, argv[0], argv[1]))
    except KeyError:
        print(f"no {argv[0]} pin for `{argv[1]}` in {CONFIG}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
