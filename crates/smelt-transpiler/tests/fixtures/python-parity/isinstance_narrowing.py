class Shape:
    def area(self) -> float:
        return 0.0


class Square(Shape):
    def __init__(self, side: float) -> None:
        self.side = side

    def area(self) -> float:
        return self.side * self.side


class Other:
    pass


def describe(x: int | str) -> str:
    if isinstance(x, int):
        return "int " + str(x + 1)
    return "str " + x.upper()


def early(x: int | str | list[int]) -> str:
    if not isinstance(x, str):
        return "number"
    return x + "!"


def maybe(x: int | None) -> int:
    if isinstance(x, int):
        return x * 2
    return -1


def classes(s: Shape) -> str:
    if isinstance(s, Square):
        return "square"
    return "shape"


print(describe(1), describe("a"))
nums: list[int] = [2]
print(early(1), early(nums), early("hi"))
absent: int | None = None
present: int | None = 4
print(maybe(present), maybe(absent))
print(isinstance(3, str), isinstance(3, int), isinstance(True, int), isinstance(2.0, (int, float)))
print(isinstance(Square(2.0), Shape), isinstance(Other(), Shape), isinstance([1], list), isinstance({}, dict))
shapes: list[Shape] = [Square(1.0), Shape()]
print([classes(s) for s in shapes])


def chain(x: int | str | list[int]) -> str:
    if isinstance(x, int):
        return "int:" + str(x * 3)
    elif isinstance(x, str):
        return "str:" + x.lower()
    else:
        return "list:" + str(len(x))


def both(x: int | str, y: int | str) -> int:
    if isinstance(x, int) and isinstance(y, int):
        return x + y
    return 0


def total(values: list[int | str]) -> int:
    acc = 0
    for value in values:
        if not isinstance(value, int):
            continue
        acc += value
    return acc


def flag(x: int | str) -> bool:
    result = isinstance(x, str)
    return result


print(chain(5), chain("ABC"), chain(nums))
print(both(2, 3), both(2, "3"))
mixed: list[int | str] = [1, "a", 4]
print(total(mixed), flag("s"), flag(1))


class Cube(Square):
    pass


solids: list[Shape] = [Cube(2.0), Square(1.0), Shape()]
print([classes(s) for s in solids])
