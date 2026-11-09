"""Programs whose frontend tests pin shape: each section prints what the shape must do."""

from abc import ABC, abstractmethod
from typing import Generic, TypeVar, TypeAlias, Union


def collected(xs: list[int]) -> list[int]:
    return list(x for x in xs)


def chars(text: str) -> list[str]:
    return [c for c in text]


class Base:
    x: int

    def __init__(self, x: int) -> None:
        self.x = x

    def value(self) -> int:
        return self.x


class Child(Base):
    y: int

    def __init__(self, x: int, y: int) -> None:
        super().__init__(x)
        self.y = y

    def total(self) -> int:
        return self.value() + self.y


class Shape(ABC):
    @abstractmethod
    def sides(self) -> int:
        pass


class Square(Shape):
    def sides(self) -> int:
        return 4


T = TypeVar("T")


class Box(Generic[T]):
    value: T

    def __init__(self, value: T) -> None:
        self.value = value


V = TypeVar("V", bound=int)


def identity(value: V) -> V:
    return value


class Ops:
    @staticmethod
    def start() -> int:
        return Ops.base() + 1

    @staticmethod
    def base() -> int:
        return 10


class MathUtils:
    @staticmethod
    def square(value: float) -> float:
        return value * value


class Inner:
    def a(self) -> int:
        return 1


class FromParam:
    def __init__(self, inner: Inner) -> None:
        self.inner = inner

    def b(self) -> int:
        return self.inner.a() + 1


class FromAnnotation:
    def __init__(self) -> None:
        self.inner: Inner = Inner()

    def b(self) -> int:
        return self.inner.a() + 2


class Good:
    def is_ok(self) -> bool:
        return True


class Bad:
    def is_ok(self) -> bool:
        return False


ExplicitAlias: TypeAlias = Union[Good, Bad]
BareAlias = Good | Bad
type StatementAlias = Good | Bad


def check_explicit(r: ExplicitAlias) -> bool:
    return r.is_ok()


def check_bare(r: BareAlias) -> bool:
    return r.is_ok()


def check_statement(r: StatementAlias) -> bool:
    return r.is_ok()


def find(x: int) -> str | None:
    if x > 0:
        return "found"
    return None


class Dog:
    name: str

    def __init__(self, name: str) -> None:
        self.name = name


d: Dog = Dog("Rex")
x: int = 1

print(collected([1, 2, 3]))
print(chars("héy"))
print(Child(2, 3).total())
print(Square().sides())
print(Box("boxed").value)
print(identity(7))
print(Ops.start())
print(MathUtils.square(1.5))
print(FromParam(Inner()).b())
print(FromAnnotation().b())
print(check_explicit(Good()), check_bare(Bad()), check_statement(Good()))
print(find(1), find(0))
print(d.name)
print(x)
