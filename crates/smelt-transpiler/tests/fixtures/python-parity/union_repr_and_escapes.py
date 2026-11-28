# A container mixing two classes prints each element's `repr` (CPython's
# `[A(1), B()]`), never Rust's `Debug`; `repr(str)` escapes exactly what
# `str.isprintable()` rejects, each in CPython's width.


class A:
    def __init__(self, x: int) -> None:
        self.x = x

    def __repr__(self) -> str:
        return f"A({self.x})"


class B:
    def __repr__(self) -> str:
        return "B()"


xs = [A(1), B()]
print(xs)
print([[A(2), B()], [B()]])
print({"a": A(3), "b": B()})
print(repr(xs), str(xs))
print((A(4), B()))

texts = [
    "tab\there\nnew\rret",
    "ctl\x01\x1f\x7f\x80\x9f",
    "nbsp\xa0soft\xadé",
    "zero​width line　ideo",
    "private max\U0010ffff emoji😀",
    "quote's",
    "both'\"",
    "back\\slash",
]
for text in texts:
    print(repr(text))
print(texts)
