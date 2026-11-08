# Module variables are one shared binding: a function's `global` statement
# writes it, every reader sees the write. A defaulted parameter takes its
# default when the call omits it, and a non-literal default is evaluated once,
# when the function is defined. A generic function keeps its type parameter,
# and `str * int` repeats the string.

from typing import TypeVar

counter = 0
names: list[str] = []

T = TypeVar("T")


def bump(step: int = 1) -> int:
    global counter
    counter += step
    return counter


def remember(name: str) -> None:
    names.append(name)


def label(text: str, width: int = 6, fill: str = ".") -> str:
    return text + fill * (width - len(text))


def first(items: list[T]) -> T:
    return items[0]


def pair[U](a: U, b: U) -> list[U]:
    return [a, b]


bump()
bump(5)
remember("ada")
remember("grace")
print(counter, names, len(names))
print(label("ab"), label("abc", 4), label("x", 3, "-"), "ab" * -1 + "|", 3 * "xy")
print(first([4, 5]), first(["p", "q"]), pair(1.5, 2.5), pair("a", "b"))
