class Base:
    def __init__(self, x: int) -> None:
        self.x = x

    def fetch(self) -> int:
        return self.x

    def greet(self, value: int) -> int:
        return value + 1


class Inherits(Base):
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

    def total(self) -> int:
        return self.fetch() + self.y


class Overrides(Base):
    def fetch(self) -> int:
        return 99

    def greet(self, value: int) -> int:
        return super().greet(value) + 10


class Middle(Base):
    def __init__(self, x: int, y: int) -> None:
        super().__init__(x)
        self.y = y


class Leaf(Middle):
    def __init__(self) -> None:
        super().__init__(1, 2)
        self.z = 3


class Wrapper:
    def __init__(self, value: int) -> None:
        self._value = value

    @property
    def wrapped(self) -> int:
        return self._value


class MathUtils:
    PI = 3

    @staticmethod
    def square(value: float) -> float:
        return value * value


def area(radius: float) -> float:
    return MathUtils.square(radius) * MathUtils.PI


print(Inherits(2, 3).total())
overrides = Overrides(5)
print(overrides.fetch(), overrides.greet(1))
leaf = Leaf()
print(leaf.x, leaf.y, leaf.z)
print(Wrapper(7).wrapped)
print(area(2.0))
