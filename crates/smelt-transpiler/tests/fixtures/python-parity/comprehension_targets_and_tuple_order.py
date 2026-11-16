# A comprehension binds a tuple target as `for` does, and tuples order
# lexicographically in `sorted`, `min` and `max`.


def main() -> None:
    d: dict[str, int] = {"b": 1, "a": 2}
    print([k + str(v) for k, v in d.items()], {v: k for k, v in d.items()}, [x * y for x, y in [(1, 2), (3, 4)]])
    print([a + b + c for (a, b), c in [((1, 2), 3)]], {s for s, _ in [("p", 1), ("q", 2), ("p", 3)]} == {"p", "q"})
    pairs: list[tuple[int, str]] = [(2, "b"), (1, "z"), (2, "a")]
    print(sorted(pairs), sorted(d.items()), max(pairs), min(pairs), sorted(pairs, reverse=True))
    scored: list[tuple[float, str]] = [(0.5, "x"), (0.25, "y"), (0.5, "a")]
    print(sorted(scored), max(scored))


main()
