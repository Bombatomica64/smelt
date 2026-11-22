def f(x: int | None = None) -> int:
    if x is None:
        return -1
    return x * 2


def g(s: str | None) -> str:
    return "none" if s is None else s.upper()


def h(xs: list[int] | None, scale: int = 1) -> int:
    if xs is not None:
        return sum(xs) * scale
    return 0


def u(v: int | str | None) -> str:
    if None is v:
        return "absent"
    return "present"


print(f(None), f(3), f(), f(x=None))
print(g(None), g("hi"))
print(h(None), h([1, 2], 3), h(None, scale=5))
print(u(None), u(1))


def first_or(items: list[str] | None, fallback: str) -> str:
    if items is not None and len(items) > 0:
        return items[0]
    return fallback


def total(v: int | list[int]) -> int:
    if isinstance(v, int):
        return v
    return sum(v)


def widen(v: int | float) -> str:
    return str(v)


print(first_or(None, "none"), first_or(["x", "y"], "none"), first_or([], "empty"))
print(total(4), total([1, 2, 3]), total([]))
print(widen(2), widen(2.5))


def empty_or_none(items: list[int] | None) -> bool:
    return items is None or len(items) == 0


print(empty_or_none(None), empty_or_none([]), empty_or_none([1]))
