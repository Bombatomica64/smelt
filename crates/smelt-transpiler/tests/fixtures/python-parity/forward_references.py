"""A function or method body reads definitions written after it.

Python resolves the names a body reads when the body runs, so a body may
construct, annotate, call and read the members of any class or function the
module defines, wherever it is written.
"""


def make_point(x: int, y: int) -> "Point":
    return Point(x, y)


def norm(p: "Point") -> int:
    return p.x * p.x + p.y * p.y


def describe(p: "Point") -> str:
    origin = Point.origin()
    return f"{p} total={p.total()} origin={origin.x},{origin.y} scaled={scale(p.x)}"


class Shape:
    def area(self) -> int:
        return 0

    def label(self) -> str:
        return f"{kind_of(self.area())} with area {self.area()}"


class Point:
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

    def total(self) -> int:
        return self.x + self.y + scale(self.x)

    def to_box(self) -> "Box":
        return Box(self, Point(self.x + 1, self.y + 1))

    @staticmethod
    def origin() -> "Point":
        return Point(0, 0)

    def __str__(self) -> str:
        return f"Point({self.x}, {self.y})"


class Box(Shape):
    def __init__(self, low: Point, high: Point) -> None:
        self.low = low
        self.high = high

    def area(self) -> int:
        return (self.high.x - self.low.x) * (self.high.y - self.low.y)


class Registry:
    def __init__(self) -> None:
        self.items: list["Entry"] = []

    def add(self, name: str) -> "Entry":
        entry = Entry(name, self)
        self.items.append(entry)
        return entry

    def size(self) -> int:
        return len(self.items)

    def entry_named(self, name: str) -> "Entry | None":
        for entry in self.items:
            if entry.name == name:
                return entry
        return None


class Entry:
    def __init__(self, name: str, owner: Registry) -> None:
        self.name = name
        self.owner = owner

    def position(self) -> int:
        return self.owner.size()


def lookup(registry: Registry, name: str) -> int:
    found = registry.entry_named(name)
    if found is None:
        return -1
    return found.position()


def kind_of(area: int) -> str:
    return "box" if area > 0 else "shape"


def scale(v: int) -> int:
    return v * SCALE


def is_even(n: int) -> bool:
    return n == 0 or is_odd(n - 1)


def is_odd(n: int) -> bool:
    return n != 0 and is_even(n - 1)


SCALE = 10

p = make_point(3, 4)
print(norm(p))
print(describe(p))
print(p.to_box().area(), p.to_box().label())
print(Shape().label())
registry = Registry()
registry.add("a")
second = registry.add("b")
print(second.name, second.position())
print(lookup(registry, "a"), lookup(registry, "z"))
print(is_even(10), is_odd(7), is_even(3))
