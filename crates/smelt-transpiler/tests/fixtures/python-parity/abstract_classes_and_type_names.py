from abc import ABC, abstractmethod


class Shape(ABC):
    @abstractmethod
    def area(self) -> float: ...

    @abstractmethod
    def label(self) -> str:
        """The shape's name."""


class Square(Shape):
    def __init__(self, side: float) -> None:
        self.side = side

    def area(self) -> float:
        return self.side * self.side

    def label(self) -> str:
        return "square " + str(self.side)


class Circle(Shape):
    def __init__(self, r: float) -> None:
        self.r = r

    def area(self) -> float:
        return 3.0 * self.r * self.r

    def label(self) -> str:
        return "circle"


class Animal:
    def describe(self) -> str:
        return type(self).__name__ + " says " + self.sound()

    def sound(self) -> str:
        return "..."


class Dog(Animal):
    def sound(self) -> str:
        return "woof"


class Cat(Animal):
    def sound(self) -> str:
        return "meow"


def main() -> None:
    shapes: list[Shape] = [Square(2.5), Circle(1.5), Square(0.5)]
    print(sum(s.area() for s in shapes))
    print([s.label() for s in shapes])
    print([type(s).__name__ for s in shapes])
    pets: list[Animal] = [Animal(), Dog(), Cat()]
    for pet in pets:
        print(pet.describe(), pet.__class__.__name__)
    print(type(3).__name__, type(2.5).__name__, type("s").__name__, type(True).__name__)
    print(type([1]).__name__, type({"a": 1}).__name__, type(None).__name__, type((1, "a")).__name__)


main()
