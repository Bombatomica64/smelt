from typing import Any, Callable


def apply(f: Callable[..., int], x: int) -> int:
    return f(x, 1)


def show(v: Any) -> None:
    print(v)


def run() -> None:
    def add(a: int, b: int) -> int:
        return a + b

    print(apply(add, 2))
    show(3)
    show("hi")
    show(True)


run()
