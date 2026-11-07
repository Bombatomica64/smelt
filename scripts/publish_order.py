#!/usr/bin/env python3
"""Answer, from `cargo metadata`, what can be published and in which order.

    publish_order.py            publishable crates, each after its path dependencies
    publish_order.py --blocked  publishable crates that cannot be published, with why

A crate is blocked when a normal or build path dependency is `publish = false`,
or is blocked itself. Dev dependencies do not count: cargo strips them.
"""

import json
import subprocess
import sys


def path_dependencies(package, members):
    """Names of the workspace members `package` needs at build or run time."""
    return sorted({
        dependency["name"]
        for dependency in package["dependencies"]
        if dependency["kind"] != "dev" and dependency["name"] in members
    })


def plan(packages):
    """Return (publish order, {blocked crate: the dependency blocking it})."""
    members = {package["name"]: package for package in packages}
    unpublishable = {name for name, package in members.items() if package["publish"] == []}
    order = []
    blocked = {}
    placed = set(unpublishable)
    pending = sorted(set(members) - unpublishable)
    while pending:
        ready = [
            name for name in pending
            if set(path_dependencies(members[name], members)) <= placed
        ]
        if not ready:
            raise SystemExit(f"path dependency cycle among {pending}")
        for name in ready:
            placed.add(name)
            pending.remove(name)
            stuck = [
                dependency for dependency in path_dependencies(members[name], members)
                if dependency in unpublishable or dependency in blocked
            ]
            if stuck:
                blocked[name] = stuck[0]
            else:
                order.append(name)
    return order, blocked


def main(argv):
    """CLI entry point; returns the process exit status."""
    metadata = json.loads(subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True, capture_output=True, text=True,
    ).stdout)
    order, blocked = plan(metadata["packages"])
    if argv == ["--blocked"]:
        for name, dependency in sorted(blocked.items()):
            print(f"{name} {dependency}")
    else:
        print("\n".join(order))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
