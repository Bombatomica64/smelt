from typing import Protocol


class Named(Protocol):
    name: str


class Shape(Named, Protocol):
    def area(self) -> int: ...

    def label(self, prefix: str) -> str: ...


class Square:
    def __init__(self, side: int) -> None:
        self.side = side
        self.name = "square"

    def area(self) -> int:
        return self.side * self.side

    def label(self, prefix: str) -> str:
        return prefix + self.name


class Rect:
    def __init__(self, w: int, h: int) -> None:
        self.w = w
        self.h = h
        self.name = "rect"

    def area(self) -> int:
        return self.w * self.h

    def label(self, prefix: str) -> str:
        return prefix + self.name + " " + str(self.w) + "x" + str(self.h)


def describe(shape: Shape) -> str:
    return shape.label("a ") + " of area " + str(shape.area())


def biggest(a: Shape, b: Shape) -> Shape:
    if a.area() >= b.area():
        return a
    return b


def run() -> None:
    print(describe(Square(3)))
    print(describe(Rect(2, 5)))
    s: Shape = Square(4)
    print(s.name)
    print(biggest(Square(3), Rect(2, 5)).name)


run()
