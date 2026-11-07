# User exception classes and builtin ones: `raise`, `except C as e`,
# `except (A, B)`, a base class catching a subclass, a bare `except`, a bare
# re-raise, `else`, `finally`, `e.args` and `str(e)` as `print` writes it.


class AppError(Exception):
    """The base of this program's errors."""


class NotFound(AppError):
    pass


class Invalid(AppError):
    ...


def find(key: str) -> int:
    if key == "missing":
        raise NotFound("no such key", key)
    if key == "bad":
        raise Invalid(key)
    if key == "value":
        raise ValueError("plain value error")
    if key == "empty":
        raise AppError
    return len(key)


for key in ["ok", "missing", "bad", "value", "empty"]:
    try:
        print("found", find(key))
    except NotFound as e:
        print("not found:", e, e.args)
    except (Invalid, KeyError) as e:
        print("invalid or key:", e, e.args)
    except AppError as e:
        print("other app error:", e, e.args, [e])
    except Exception as e:
        print("exception:", e)
    finally:
        print("finally", key)

try:
    try:
        find("bad")
    except Invalid:
        print("inner caught, re-raising")
        raise
except AppError as e:
    print("outer caught", e)

try:
    find("missing")
except ValueError:
    print("wrong handler")
except AppError as e:
    print("base catches subclass:", e)

try:
    try:
        find("value")
    finally:
        print("finally runs before the outer handler")
except ValueError as e:
    print("propagated through finally:", e)

try:
    find("missing")
except:
    print("bare except catches everything")

try:
    try:
        find("value")
    except AppError:
        print("wrong: no clause names ValueError")
except ValueError as e:
    print("unmatched exception propagates:", e)

counts: dict[str, int] = {"a": 1}
try:
    print(counts["zz"])
except KeyError as e:
    print("key error", e, e.args)

try:
    zero = 0
    print(10 // zero)
except ArithmeticError as e:
    print("arithmetic", e)


def check(n: int) -> None:
    try:
        if n > 1:
            raise RuntimeError("too big")
    except RuntimeError as e:
        print("caught in function", e)
    else:
        print("no error for", n)
    finally:
        print("done", n)


check(1)
check(5)


def else_raises() -> None:
    try:
        print("try body")
    except ValueError:
        print("wrong: else is not protected by its own handlers")
    else:
        raise ValueError("raised in else")


try:
    else_raises()
except ValueError as e:
    print("else raise propagated:", e)

try:
    raise Invalid("with cause") from None
except AppError as e:
    print("from None:", e)


class Countdown:
    def __init__(self, start: int, fail_at: int) -> None:
        self.n = start
        self.fail_at = fail_at

    def __iter__(self) -> "Countdown":
        return self

    def __next__(self) -> int:
        if self.n == self.fail_at:
            raise Invalid("countdown broke at", self.n)
        if self.n == 0:
            raise StopIteration
        self.n -= 1
        return self.n + 1


for x in Countdown(3, -1):
    print("tick", x)
try:
    for x in Countdown(5, 2):
        print("tick", x)
except StopIteration:
    print("wrong: the loop's StopIteration escaped")
except Invalid as e:
    print("iterator error propagated:", e)
