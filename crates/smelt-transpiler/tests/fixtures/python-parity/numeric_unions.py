# A value of a union type runs each operation on the member it holds:
# arithmetic, comparisons, `abs`, `round` and `-` on an `int | float`
# (`int` with `int` stays exact, anything with a `float` is a `float`), and
# `isinstance` / `is None` on an `int | str | None`.


def pick(a: int, b: float) -> int | float:
    return max(a, b)


def scale(v: int | float) -> float:
    return v * 2.5


def bump(v: int | float) -> int | float:
    return v + 1


def show() -> None:
    m = pick(3, 2.5)
    n = pick(1, 2.5)
    print(m * 2, n * 2, m + n, m // 2, n % 2, m ** 2, m - 1, m * n)
    print(m > n, m < 10, m == 3, n >= 2.5, n != 2.5, -m, -n, abs(m), abs(n))
    print(m / 2, 2 / n, m * m, round(n), round(m), str(m), f"{m}|{n}")
    t = 0.0
    for v in [m, n]:
        t += v
    print(t)


show()
print(scale(2), scale(1.5), bump(3), bump(0.5))
vals: list[int | float] = [1, 2.5]
total = 0.0
for v in vals:
    total += v
print(total)


def describe(v: int | str | None) -> str:
    if v is None:
        return "none"
    if isinstance(v, int):
        return "int " + str(v + 1)
    return "str " + v.upper()


def classify(v: int | str | None) -> str:
    if isinstance(v, str):
        return "s:" + v
    elif isinstance(v, int):
        return "i:" + str(v * 2)
    return "nothing"


items: list[int | str | None] = [1, "a", None]
for x in items:
    print(describe(x), classify(x), isinstance(x, (int, str)), x)
print(items)
