"""functools: reduce, partial, lru_cache/cache and wraps over typed functions."""

import functools
from dataclasses import dataclass
from functools import cache, lru_cache, partial, reduce, wraps
from typing import Callable


@dataclass
class Order:
    item: str
    qty: int
    price: float


def add(a: int, b: int) -> int:
    return a + b


def total_cost(acc: float, order: Order) -> float:
    return acc + order.qty * order.price


def multiply(a: int, b: int) -> int:
    return a * b


def larger(a: int, b: int) -> int:
    return a if a > b else b


def add_length(total: int, word: str) -> int:
    return total + len(word)


def busier(best: Order, order: Order) -> Order:
    return order if order.qty > best.qty else best


def power(base: int, exponent: int) -> int:
    return base**exponent


def greet(greeting: str, name: str, punct: str = "!") -> str:
    return greeting + ", " + name + punct


@lru_cache(maxsize=None)
def fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)


@cache
def edit_distance(a: str, b: str) -> int:
    if not a:
        return len(b)
    if not b:
        return len(a)
    if a[0] == b[0]:
        return edit_distance(a[1:], b[1:])
    return 1 + min(
        edit_distance(a[1:], b),
        edit_distance(a, b[1:]),
        edit_distance(a[1:], b[1:]),
    )


@functools.cache
def ways(total: int, largest: int) -> int:
    if total == 0:
        return 1
    if total < 0 or largest == 0:
        return 0
    return ways(total - largest, largest) + ways(total, largest - 1)


def shouting(func: Callable[[str], str]) -> Callable[[str], str]:
    @wraps(func)
    def wrapper(text: str) -> str:
        return func(text).upper() + "!"

    return wrapper


@functools.lru_cache
def catalan(n: int) -> int:
    if n <= 1:
        return 1
    total = 0
    for i in range(n):
        total += catalan(i) * catalan(n - 1 - i)
    return total


@lru_cache(maxsize=2)
def noisy_square(n: int) -> int:
    print("computing", n)
    return n * n


def apply_twice(f: Callable[[int, int], int], x: int) -> int:
    step = partial(f, 1)
    return step(step(x))


@shouting
def echo(text: str) -> str:
    return text + " " + text


def main() -> None:
    nums = [1, 2, 3, 4, 5]
    print(reduce(add, nums))
    print(reduce(add, nums, 100))
    print(functools.reduce(multiply, nums, 1))
    print(reduce(larger, [3, 9, 2, 7]))
    words = ["smelt", "rust", "py"]
    print(reduce(add_length, words, 0))
    orders = [Order("apple", 3, 0.5), Order("pear", 2, 1.25), Order("fig", 10, 0.2)]
    print(reduce(total_cost, orders, 0.0))
    print(reduce(busier, orders).item)

    square = partial(power, exponent=2)
    cube = partial(power, exponent=3)
    print([square(n) for n in nums])
    print(list(map(cube, nums)))
    two_to = partial(power, 2)
    print(two_to(10))
    hello = partial(greet, "Hello")
    print(hello("Ada"))
    print(hello("Bob", "?"))
    print(hello(name="Eve"))
    add_ten = functools.partial(add, 10)
    print(sorted([5, -20, 3], key=add_ten))
    print(reduce(partial(power), [2, 3, 2]))

    print(fib(30))
    print(fib(90))
    print([fib(i) for i in range(10)])
    print(edit_distance("kitten", "sitting"))
    print(edit_distance("flaw", "lawn"))
    print(ways(100, 50))
    print([catalan(n) for n in range(12)], catalan(60))

    for n in [1, 2, 1, 3, 2, 1]:
        print(noisy_square(n))

    print(apply_twice(add, 5))
    hi = partial(greet, "Hi")
    hi_zed = partial(hi, "Zed")
    print(hi_zed())
    empty: list[int] = []
    try:
        print(reduce(add, empty))
    except TypeError as error:
        print("TypeError:", error)
    print(reduce(add, empty, 7))

    print(echo("hi"))


if __name__ == "__main__":
    main()
