# Exception classes with fields and methods: typed instances raised, caught
# by their own class or a base, re-raised, and shown through `str`, `repr`
# and `args` exactly as CPython writes them.


class AppError(Exception):
    """The base of this program's errors."""


class HttpError(AppError):
    def __init__(self, status: int, url: str) -> None:
        super().__init__(f"{status} for {url}")
        self.status = status
        self.url = url

    def retryable(self) -> bool:
        return self.status >= 500


class NotFound(HttpError):
    def __init__(self, url: str) -> None:
        super().__init__(404, url)
        self.attempts = 0


class Coded(Exception):
    # `__init__` never calls `super().__init__`: `args` keeps the
    # constructor's arguments (`BaseException.__new__` stored them).
    def __init__(self, code: int, detail: str = "none") -> None:
        self.code = code
        self.detail = detail


class Pretty(Coded):
    def __str__(self) -> str:
        return f"pretty<{self.code}>"


class Wrapped(Pretty):
    def __str__(self) -> str:
        return "wrapped " + super().__str__()


class Token:
    def __init__(self, text: str) -> None:
        self.text = text


class ParseError(ValueError):
    def __init__(self, token: Token) -> None:
        super().__init__(f"unexpected {token.text!r}")
        self.token = token


class Plain(KeyError):
    pass


def fetch(url: str) -> str:
    if url.endswith("/missing"):
        raise NotFound(url)
    if url.endswith("/down"):
        raise HttpError(503, url)
    if url.endswith("/coded"):
        raise Coded(7)
    if url.endswith("/pretty"):
        raise Pretty(9, "x")
    if url.endswith("/wrapped"):
        raise Wrapped(3)
    if url.endswith("/parse"):
        raise ParseError(Token("}"))
    if url.endswith("/plain"):
        raise Plain("k")
    return "ok " + url


for url in ["a/ok", "a/missing", "a/down", "a/coded", "a/pretty", "a/wrapped", "a/parse", "a/plain"]:
    try:
        print(fetch(url))
    except NotFound as e:
        e.attempts += 1
        print("not found:", e.url, e.status, e.attempts, e, e.args, repr(e))
    except HttpError as e:
        print("http:", e.status, e.retryable(), str(e), [e])
    except (Pretty, Coded) as e:
        print("coded:", e.code, e.detail, e, e.args, repr(e))
    except ParseError as e:
        print("parse:", e.token.text, e, e.args)
    except Exception as e:
        print("other:", e, repr(e), e.args)

# A base catches every subclass, and the instance keeps its class.
try:
    fetch("b/missing")
except AppError as e:
    print("app error:", e, repr(e), isinstance(e, NotFound))

# Re-raise keeps the same instance: a change through one name shows through
# the other.
try:
    try:
        fetch("c/missing")
    except NotFound as inner:
        inner.attempts = 5
        raise
except HttpError as outer:
    print("re-raised:", outer.status, outer.url)

try:
    try:
        fetch("d/missing")
    except NotFound as inner:
        inner.attempts = 9
        raise inner
except NotFound as again:
    print("raise e keeps identity:", again.attempts)


def translate(url: str) -> str:
    try:
        return fetch(url)
    except HttpError as e:
        raise Coded(e.status, e.url) from e


try:
    translate("e/down")
except Coded as e:
    print("translated:", e.code, e.detail, e.args)

# An instance built ahead of the `raise`, and one held at its base type.
error: AppError = HttpError(500, "f/held")
try:
    raise error
except HttpError as e:
    print("held:", e.retryable(), e)

# The untyped `except Exception` sees the class's `__str__` and `repr`.
for url in ["g/wrapped", "g/missing", "g/plain"]:
    try:
        fetch(url)
    except Exception as e:
        print("generic:", e, repr(e), e.args)
