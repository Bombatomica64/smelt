"""Slice assignment (contiguous, extended, emptying, negative bounds, a
size mismatch), `zip` with one or three iterables, and `min`/`max` with
`default=`."""


def main() -> None:
    xs = [0, 1, 2, 3, 4, 5, 6, 7]
    xs[1:3] = [10, 20, 30]
    print(xs)
    xs[::2] = [-1, -2, -3, -4, -5]
    print(xs)
    xs[-2:] = [99]
    print(xs)
    xs[100:] = [7, 8]
    print(xs)
    xs[2:1] = [55]
    print(xs)
    xs[::-3] = [1, 2, 3, 4]
    print(xs)
    xs[2:] = []
    print(xs)
    alias = xs
    xs[:] = (n * n for n in range(4))
    print(alias, alias is xs)
    try:
        xs[::2] = [1, 2, 3]
    except ValueError as e:
        print("ValueError:", e)
    try:
        xs[::0] = []
    except ValueError as e:
        print("ValueError:", e)
    words = ["a", "b", "c", "d"]
    words[1:3] = words
    print(words)

    a = [1, 2, 3, 4]
    b = ["one", "two", "three"]
    c = [True, False, True, False]
    print(list(zip(a, b, c)))
    print(list(zip(a)))
    for n, name, flag in zip(a, b, c):
        print(n, name, flag)
    print([p + q + r for p, q, r in zip(a, a, a)])

    empty: list[int] = []
    print(min(empty, default=-1), max([3, 9, 2], default=0), max(empty, default=7))
    print(max(empty, default=None), min(["pear", "fig"], default="none"))
    print(max(["aa", "b", "cccc"], key=len, default=""), min(empty, key=lambda v: -v, default=0))


main()
