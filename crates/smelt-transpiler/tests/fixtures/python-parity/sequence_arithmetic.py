# `+` and `*` on lists and tuples build new sequences that share their
# elements, as CPython does; `+=` and `*=` on a list mutate it in place, so
# every alias sees the change.
def main() -> None:
    a: list[int] = [1, 2]
    b: list[int] = [3]
    c = a + b
    print(c, a, b)
    print(a * 3, 2 * b, a * 0, a * -1)
    grid = [[0]] * 3
    grid[0].append(9)
    print(grid)
    alias = a
    a += [7, 8]
    print(alias, a)
    alias2 = b
    b *= 3
    print(alias2, b)
    t = (1, "x") + (2.5,)
    print(t, (1, 2) * 2)
    words: list[str] = ["a"] + ["b", "c"]
    print(words, len(words * 2))
    nested: list[list[int]] = [[1], [2]]
    joined = nested + nested
    joined[0].append(5)
    print(nested, joined)


main()
