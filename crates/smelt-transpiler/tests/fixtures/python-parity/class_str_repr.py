# A class's `__str__` and `__repr__` are its text: `print`, `str`, f-strings
# and `repr` call them, `str` falls back to `__repr__`, and a container shows
# each element's `__repr__`.


class Point:
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

    def __repr__(self) -> str:
        return f"Point({self.x}, {self.y})"

    def __str__(self) -> str:
        return f"({self.x}, {self.y})"


class Tag:
    def __init__(self, name: str) -> None:
        self.name = name

    def __repr__(self) -> str:
        return "Tag<" + self.name + ">"


class Labelled:
    def __init__(self, label: str) -> None:
        self.label = label

    def __str__(self) -> str:
        return "label: " + self.label


class Special(Tag):
    def __init__(self, name: str, level: int) -> None:
        super().__init__(name)
        self.level = level

    def __str__(self) -> str:
        return f"{self.name}!{self.level}"


class Strict:
    def __init__(self, ok: bool) -> None:
        self.ok = ok

    def __str__(self) -> str:
        if not self.ok:
            raise ValueError("no text for this one")
        return "fine"


def main() -> None:
    p = Point(1, 2)
    print(p)
    print(str(p), repr(p), f"{p} and {p!r}")
    print([p, Point(3, 4)], (p, 5))
    tag = Tag("a")
    print(tag, str(tag), repr(tag), f"<{tag}>")
    print([tag], {"k": tag})
    labelled = Labelled("x")
    print(labelled, str(labelled), f"[{labelled}]")
    special = Special("b", 2)
    print(special, repr(special), [special])
    print(Strict(True))
    try:
        print(Strict(False))
    except ValueError as error:
        print("ValueError:", error)


main()
