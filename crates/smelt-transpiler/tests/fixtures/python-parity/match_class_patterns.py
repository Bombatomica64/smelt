"""`match` class patterns: keyword and positional sub-patterns
(`__match_args__`, a dataclass's fields), nested patterns and captures,
guards, a union subject, and a base-typed subject matched by subclass."""
from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int


class Segment:
    __match_args__ = ("start", "end")

    def __init__(self, start: Point, end: Point) -> None:
        self.start = start
        self.end = end


class Circle:
    __match_args__ = ("radius", "label")

    def __init__(self, radius: int, label: str) -> None:
        self.radius = radius
        self.label = label


class Shape:
    def area(self) -> int:
        return 0


class Square(Shape):
    __match_args__ = ("side",)

    def __init__(self, side: int) -> None:
        self.side = side

    def area(self) -> int:
        return self.side * self.side


class Rect(Shape):
    __match_args__ = ("w", "h")

    def __init__(self, w: int, h: int) -> None:
        self.w = w
        self.h = h


def where(p: Point) -> str:
    match p:
        case Point(x=0, y=0):
            return "origin"
        case Point(x=0, y=y):
            return f"y-axis {y}"
        case Point(x, 0):
            return f"x-axis {x}"
        case Point(x=x, y=y) if x == y:
            return f"diagonal {x}"
        case Point(x=1 | 2 as small, y=y):
            return f"small x {small}, y {y}"
        case _:
            return "elsewhere"


def segment(s: Segment) -> str:
    match s:
        case Segment(Point(0, 0), Point(x, y)):
            return f"from origin to {x},{y}"
        case Segment(start=Point(x=a), end=Point(x=b)) if a == b:
            return f"vertical at {a}"
        case Segment(start, end):
            return f"{start.x},{start.y} -> {end.x},{end.y}"
    return "unreachable"


def describe(item: Circle | Point) -> str:
    match item:
        case Circle(0, label):
            return f"dot {label}"
        case Circle(radius=r, label="big") if r > 10:
            return f"big circle {r}"
        case Circle(r, label):
            return f"circle {r} {label}"
        case Point(x, y):
            return f"point {x},{y}"
    return "unreachable"


def shape(s: Shape) -> str:
    match s:
        case Square(1):
            return "unit square"
        case Square(side):
            return f"square {side} area {s.area()}"
        case Rect(w, h) if w == h:
            return f"square-ish rect {w}"
        case Rect(w=w):
            return f"rect width {w}"
        case Shape():
            return "some shape"
    return "unreachable"


def main() -> None:
    for p in [Point(0, 0), Point(0, 5), Point(3, 0), Point(2, 2), Point(1, 7), Point(5, 6)]:
        print(where(p))
    print(segment(Segment(Point(0, 0), Point(3, 4))))
    print(segment(Segment(Point(2, 1), Point(2, 9))))
    print(segment(Segment(Point(1, 1), Point(2, 2))))
    things: list[Circle | Point] = [Circle(0, "a"), Circle(20, "big"), Circle(3, "c"), Point(4, 5)]
    for thing in things:
        print(describe(thing))
    shapes: list[Shape] = [Square(1), Square(3), Rect(2, 2), Rect(4, 1), Shape()]
    for s in shapes:
        print(shape(s))


main()
