"""`throw()`, `close()`, `send()` to a just-started generator, and `StopIteration.value`."""
from typing import Generator, Optional


def counter() -> Generator[int, Optional[int], str]:
    n = 0
    try:
        while True:
            try:
                got = yield n
                if got is not None:
                    n = got
                else:
                    n += 1
            except ValueError as e:
                print("caught", e)
                n = -1
    finally:
        print("counter cleanup")


def worker() -> Generator[int, None, None]:
    try:
        yield 1
        yield 2
    except GeneratorExit:
        print("worker closing")
        raise
    finally:
        print("worker finally")


def stubborn() -> Generator[int, None, None]:
    try:
        yield 1
    except GeneratorExit:
        yield 99


def strict() -> Generator[int, int, None]:
    total = 0
    while True:
        total += yield total


g = counter()
print(next(g))
print(g.send(10))
print(next(g))
print(g.throw(ValueError("bad")))
print(next(g))
g.close()
print("closed")
try:
    next(g)
except StopIteration:
    print("exhausted after close")

w = worker()
print(next(w))
w.close()
w.close()
print("worker done")

fresh = worker()
fresh.close()
print("fresh closed without running")

s = stubborn()
next(s)
try:
    s.close()
except RuntimeError as e:
    print("runtime error:", e)

t = strict()
try:
    t.send(5)
except TypeError as e:
    print("type error:", e)
print(next(t))
print(t.send(5))
print(t.send(7))

u = counter()
try:
    u.throw(KeyError("early"))
except KeyError as e:
    print("raised at start:", repr(e))
try:
    next(u)
except StopIteration:
    print("a generator thrown into at start is done")

v = worker()
next(v)
try:
    v.throw(IndexError)
except IndexError:
    print("propagated IndexError")


def summed(limit: int) -> Generator[int, None, str]:
    total = 0
    for i in range(limit):
        total += i
        yield i
    return f"sum={total}"


def drain(limit: int) -> str:
    gen = summed(limit)
    while True:
        try:
            next(gen)
        except StopIteration as stop:
            return str(stop.value)


def bare() -> Generator[int, None, None]:
    yield 1


print(drain(4))
b = bare()
next(b)
try:
    next(b)
except StopIteration as stop:
    print("value:", stop.value, repr(stop.value), f"<{stop.value}>")
try:
    raise StopIteration("explicit")
except StopIteration as stop:
    print(repr(stop.value), stop.args)


def guarded() -> Generator[str, None, None]:
    for word in ["a", "b", "c"]:
        try:
            yield word
        except ValueError as e:
            print("skipping after", e)
        finally:
            print("after", word)


h = guarded()
print(next(h))
print(h.throw(ValueError("x")))
h.close()
print("guarded closed")
