"""Field narrowing, subclass narrowing, mixed conditional expressions, and
user methods spelled like builtin methods."""


class Entry:
    def __init__(self, name: str) -> None:
        self.name = name
        self.owner: "Registry | None" = None

    def position(self) -> int:
        if self.owner is None:
            return -1
        return self.owner.index_of(self.name)

    def label(self) -> str:
        return self.name.upper() if self.owner is not None else self.name


class Registry:
    def __init__(self) -> None:
        self.entries: list[Entry] = []

    def add(self, name: str) -> Entry:
        entry = Entry(name)
        entry.owner = self
        self.entries.append(entry)
        return entry

    def find(self, name: str) -> Entry | None:
        for entry in self.entries:
            if entry.name == name:
                return entry
        return None

    def items(self) -> list[str]:
        return [entry.name for entry in self.entries]

    def index_of(self, name: str) -> int:
        return self.items().index(name)

    def count(self) -> int:
        return len(self.entries)


class Shape:
    def area(self) -> int:
        return 0

    def kind(self) -> str:
        return "shape"


class Square(Shape):
    def __init__(self, side: int) -> None:
        self.side = side

    def area(self) -> int:
        return self.side * self.side


class Circle(Shape):
    def kind(self) -> str:
        return "circle"


def as_square(shape: Shape) -> Square | None:
    if isinstance(shape, Square):
        return shape
    return None


def side_of(shape: Shape) -> int:
    if not isinstance(shape, Square):
        return 0
    return shape.side


def pick(registry: Registry, name: str) -> Entry | None:
    return registry.find(name) if name else None


def size(flag: bool) -> int | float:
    return 2 if flag else 2.5


registry = Registry()
first = registry.add("alpha")
registry.add("beta")
loose = Entry("loose")
print(first.position(), registry.add("gamma").position(), loose.position())
print(first.label(), loose.label())
found = registry.find("beta")
print(found is not None, registry.find("zeta") is None, registry.items(), registry.count())
print(pick(registry, "gamma") is not None, pick(registry, "") is None)
square = Square(3)
print(as_square(square) is square, as_square(Circle()) is None, side_of(square), side_of(Shape()))
for shape in [Shape(), Square(2), Circle()]:
    print(shape.kind(), shape.area())
print(size(True), size(False))
