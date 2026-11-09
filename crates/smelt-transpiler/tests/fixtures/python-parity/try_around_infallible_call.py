# A try around a call that cannot raise runs the body and its finally.
from typing import Callable


class Marker:
    """A placeholder class."""

    ...


def apply(f: Callable[[int], int], value: int) -> int:
    return f(value)


def guarded(value: int) -> int:
    try:
        return apply(lambda x: x + 1, value)
    except ValueError as error:
        print(error)
        return 0
    finally:
        print("done")


print(guarded(41))
