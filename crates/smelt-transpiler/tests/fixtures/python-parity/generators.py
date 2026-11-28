# Generator functions: `yield` suspends, a `for` loop resumes once per
# iteration (the generator's prints interleave with the loop body's), a
# `break` leaves the rest unproduced, and `list`/`sum`/`sorted`/`max`
# drain one. Generator expressions consumed by `any`/`all` stop at the
# first deciding element.
from typing import Generator, Iterator


def count(n: int) -> Iterator[int]:
    i = 0
    while i < n:
        print("  produce", i)
        yield i
        i += 1
    print("  exhausted")


def words(text: str) -> Generator[str, None, None]:
    for word in text.split():
        if word == "stop":
            return
        yield word.upper()


for v in count(3):
    print("consume", v)
for v in count(10):
    if v == 1:
        break
    print("first", v)
else:
    print("wrong: the loop broke")
print(list(count(2)), sum(count(4)))
print(list(words("a bb stop ccc")), sorted(words("b a c")), max(count(3)))


def failing(limit: int) -> Iterator[int]:
    yield 1
    if limit < 2:
        raise ValueError("limit too small")
    yield 2


try:
    for v in failing(1):
        print("got", v)
except ValueError as e:
    print("caught", e)

seen: list[int] = []


def big(x: int) -> bool:
    seen.append(x)
    return x > 2


xs = [1, 3, 5, 0]
print(any(big(x) for x in xs), seen)
seen.clear()
print(all(not big(x) for x in xs if x != 1), seen)
print(sum(x * x for x in xs), max(x for x in xs), any(x for x in [0, 0]), all(s for s in ["a", ""]))
