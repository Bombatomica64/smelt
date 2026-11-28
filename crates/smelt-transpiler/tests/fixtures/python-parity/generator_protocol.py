# The generator protocol: `send()` resumes the `yield` with a value,
# `return value` is the `StopIteration`'s value and `yield from`'s result,
# `next(g[, default])` resumes once, and `zip`/`enumerate`/`any`/`all` over a
# generator object pull one value at a time, as CPython does (each
# generator's prints interleave with its consumer's; `zip` pulls its first
# operand once more after a shorter second one runs out).
from typing import Generator, Iterator


def averager() -> Generator[float, float, str]:
    total = 0.0
    count = 0
    average = 0.0
    while True:
        value = yield average
        if value < 0:
            break
        total += value
        count += 1
        average = total / count
    return f"done after {count}"


def inner(n: int) -> Generator[int, None, str]:
    for i in range(n):
        yield i
    return "inner" + str(n)


def outer() -> Generator[int, None, int]:
    a = yield from inner(2)
    print("got", a)
    yield from [10, 20]
    b = yield from inner(1)
    print("got", b)
    return 7


def letters(word: str) -> Iterator[str]:
    yield from word


g = averager()
print(next(g))
print(g.send(10.0))
print(g.send(20.0))
try:
    g.send(-1.0)
except StopIteration as e:
    print("stop", e, e.args)

print(list(outer()), list(letters("ab")))
o = outer()
print(next(o), next(o), next(o))
try:
    while True:
        print("v", next(o))
except StopIteration as e:
    print("outer stop", e)
c = inner(1)
print(next(c, -1), next(c, -1), next(c, -1))


def plain() -> Iterator[int]:
    yield 1


p = plain()
print(next(p))
try:
    next(p)
except StopIteration as e:
    print("plain stop", repr(e), e.args)


def count(n: int) -> Iterator[int]:
    i = 0
    while i < n:
        print("  produce", i)
        yield i
        i += 1
    print("  exhausted")


for i, v in enumerate(count(3), 10):
    print("enum", i, v)
for a, s in zip(count(5), ["a", "b"]):
    print("zip gl", a, s)
for s, a in zip(["x", "y", "z"], count(2)):
    print("zip lg", s, a)
for a, b in zip(count(2), count(3)):
    print("zip gg", a, b)
print(any(count(5)))
print(all(count(3)))
e = enumerate(count(2))
print(next(e))
print(list(zip(count(3), "pq")))
