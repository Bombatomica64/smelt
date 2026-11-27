# An override family shares one signature: a base method that raises and an
# override that does not (or the reverse) are both callable from an inherited
# method, whichever class's instance runs it. An ABC's concrete method calls
# its abstract method on `self`.
from abc import ABC, abstractmethod


class Shape(ABC):
    @abstractmethod
    def area(self) -> float: ...

    def describe(self) -> str:
        return f"{type(self).__name__} area {self.area()}"


class Square(Shape):
    def __init__(self, side: float) -> None:
        self.side = side

    def area(self) -> float:
        return self.side * self.side


class Strict(Square):
    def area(self) -> float:
        if self.side < 0:
            raise ValueError("negative side")
        return super().area()


class Named(ABC):
    # No override raises: only the stub would, and no dispatch reaches it.
    @abstractmethod
    def name(self) -> str: ...

    def greet(self) -> str:
        return "hi " + self.name()


class Bob(Named):
    def name(self) -> str:
        return "bob"


class Base:
    def f(self, x: int) -> int:
        if x < 0:
            raise ValueError("negative")
        return x

    def g(self, x: int) -> int:
        return self.f(x) + 1


class Child(Base):
    def f(self, x: int) -> int:
        return x * 2


class Quiet:
    def f(self) -> str:
        return "quiet"

    def twice(self) -> str:
        return self.f() + self.f()


class Loud(Quiet):
    def f(self) -> str:
        raise RuntimeError("too loud")


shapes: list[Shape] = [Square(2.0), Strict(3.0), Strict(-1.0)]
for s in shapes:
    try:
        print(s.describe())
    except ValueError as e:
        print("refused:", e)
print(Child().g(3), Base().g(3), Bob().greet())
try:
    Base().g(-1)
except ValueError as e:
    print("caught", e)
print(Quiet().twice())
try:
    print(Loud().twice())
except RuntimeError as e:
    print("caught", e)
