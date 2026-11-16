# A `@dataclass` gets the `__repr__` the decorator generates
# (`Point(x=1, label='a')`), shown by print, repr, f-strings and containers,
# unless the class defines its own.
from dataclasses import dataclass


@dataclass
class Point:
    x: int
    label: str = "origin"


@dataclass
class Custom:
    value: float

    def __repr__(self) -> str:
        return f"<{self.value}>"


def main() -> None:
    p = Point(1, "a")
    print(p, repr(p), f"{p}", [p, Point(2 ** 70)], Point(1, "a") == p)
    print(Custom(1.5), [Custom(2.0)])


main()
