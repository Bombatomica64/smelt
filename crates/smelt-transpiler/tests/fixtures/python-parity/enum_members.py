"""`enum.Enum`: singleton members with `name`/`value`, `auto()`, `str`/
`repr`, iteration and `len`, lookup by value (`ValueError` for none),
identity and equality, membership in a list, a method in the class
body, and `match` on members."""
from enum import Enum, auto


class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

    def is_warm(self) -> bool:
        return self is Color.RED

    def lower(self) -> str:
        return self.name.lower()


class Mode(Enum):
    READ = auto()
    WRITE = auto()
    APPEND = 10
    EXCLUSIVE = auto()


class Planet(Enum):
    MERCURY = "hot"
    NEPTUNE = "cold"


def describe(c: Color) -> str:
    match c:
        case Color.RED:
            return "stop"
        case Color.GREEN:
            return "go"
        case _:
            return "other"


def main() -> None:
    c = Color.GREEN
    print(c.name, c.value, c == Color.GREEN, c is Color.RED, c != Color.BLUE)
    print([m.name for m in Color], len(Color))
    print(Color(3).name, Color(1) is Color.RED)
    print(c, repr(c), [c, Color.BLUE], f"{c}")
    print([m.value for m in Mode], Mode.EXCLUSIVE)
    print(Planet.NEPTUNE.value, repr(Planet.MERCURY), Planet("hot").name)
    print(Color.RED.is_warm(), c.is_warm(), c.lower())
    print([describe(m) for m in Color])
    try:
        Color(7)
    except ValueError as e:
        print("ValueError:", e)
    try:
        Planet("lukewarm")
    except ValueError as e:
        print("ValueError:", e)
    seen: list[Color] = []
    for m in Color:
        if m not in seen:
            seen.append(m)
    print(len(seen), seen.index(Color.BLUE))


main()
