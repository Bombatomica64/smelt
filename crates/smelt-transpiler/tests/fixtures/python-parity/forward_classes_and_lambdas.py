# `isinstance` names a class the module defines further down (Python resolves
# the name when the call runs); a subclass instance is passed where its base
# is declared; a lambda is called where it is written, or bound unannotated to
# a name and typed by each call.
from typing import Callable


class Base:
    def name(self) -> str:
        return "base"


def kind(v: Base) -> str:
    if isinstance(v, Leaf):
        return "leaf"
    if isinstance(v, Mid):
        return "mid"
    return "base"


def is_base(v: Mid) -> bool:
    return isinstance(v, Base)


def describe(v: Base) -> str:
    return v.name() + "/" + kind(v)


class Mid(Base):
    def name(self) -> str:
        return "mid"


class Leaf(Mid):
    def name(self) -> str:
        return "leaf"


print(kind(Leaf()), kind(Mid()), kind(Base()), is_base(Leaf()), is_base(Mid()))
print(describe(Leaf()), describe(Base()))
shapes: list[Base] = [Leaf(), Base(), Mid()]
print([isinstance(s, Mid) for s in shapes])

print((lambda x: x + 1)(2))
print((lambda a, b: f"{a}-{b}")("x", 3.5))
print((lambda: "none")())
k = 10
print((lambda x, y=k: x * y)(4))


def twice(n: int) -> int:
    return (lambda v: v * 2)(n)


print(twice(21))

square = lambda n: n * n
print(square(4), square(1.5))
offset = 3
add_offset = lambda v: v + offset
print(add_offset(1), add_offset(2.5))
offset = 10
print(add_offset(1))


def local_use() -> None:
    fmt = lambda a, b: f"{a}={b}"
    print(fmt("x", 1), fmt(2, [3]))


local_use()


def apply(f: Callable[[int], int], v: int) -> int:
    return f(v)


xs = [3, -7, 2]
print(apply(lambda n: n + 100, 5), sorted(xs, key=lambda v: abs(v)))
print(list(map(lambda v: v * 2, xs)), list(filter(lambda v: v > 0, xs)))
