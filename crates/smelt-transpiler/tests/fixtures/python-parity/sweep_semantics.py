"""Small semantics a probe sweep found diverging: print's sep/end/file, tuple
assignment to existing names, attributes and items, an exception raised by a
property setter, identity of a list copy, sum's start and float rounding, and
str case/space predicates."""

import sys


class Celsius:
    def __init__(self, degrees: float) -> None:
        self._degrees = degrees

    @property
    def degrees(self) -> float:
        return self._degrees

    @degrees.setter
    def degrees(self, value: float) -> None:
        if value < -273.15:
            raise ValueError(f"{value} is below absolute zero")
        self._degrees = value


class Span:
    def __init__(self, start: int, end: int) -> None:
        self.start, self.end = start, end

    def flip(self) -> None:
        self.start, self.end = self.end, self.start


def fib(n: int) -> int:
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a


def fib_while(n: int) -> int:
    a, b = 0, 1
    while n > 0:
        a, b = b, a + b
        n -= 1
    return a


def bubble(xs: list[int]) -> list[int]:
    for i in range(len(xs)):
        for j in range(len(xs) - 1 - i):
            if xs[j] > xs[j + 1]:
                xs[j], xs[j + 1] = xs[j + 1], xs[j]
    return xs


def main() -> None:
    print("a", end="")
    print("b", end=",")
    print("c")
    print("x", "y", 3, sep="-", end="!\n")
    print(1, 2.5, None, [1, 2], sep=" | ")
    for i in range(4):
        print(i, end=" ")
    print()
    print("hidden", file=sys.stderr)
    print("flushed", flush=True)

    print(fib(10), fib_while(20), bubble([5, 1, 4, 2, 3]))
    span = Span(1, 9)
    span.flip()
    print(span.start, span.end)
    x, (y, z) = 1, (2, 3)
    x, (y, z) = z, (x, y)
    print(x, y, z)

    temp = Celsius(20.0)
    try:
        temp.degrees = -300.0
    except ValueError as e:
        print("refused:", e)
    temp.degrees = 25.5
    print(temp.degrees)

    a = [1, 2]
    print(a.copy() is a, a.copy() == a, a[:] is a, a is a)

    xs = [1, 2, 3]
    print(sum(xs, 10), sum(xs, start=5), sum([0.1, 0.2], 0.3), sum([0.1] * 10), sum([1e100, 1.0, -1e100]))
    for s in ["  \t", "ABC", "abc", "Abc", "A1", "", "a b"]:
        print(repr(s), s.isspace(), s.isupper(), s.islower())


main()
