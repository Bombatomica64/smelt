# `str()`, `repr()` and f-string interpolation write Python text: quoted
# strings, `True`/`None`, float `repr`, containers built from element `repr`s,
# exceptions and their `args`.
class Bad(Exception):
    pass


nums: list[int] = [1, 2]
words: list[str] = ["it's", "b"]
print(str(nums), repr(words), str(words))
print(repr("x"), repr(1.5), repr(True), repr(None), repr(3))
print(str((1, "a")), str({"k": [1.0, 2.5]}))
e = Bad("msg", 2)
print(str(e), repr(e), str(e.args), repr(Bad("one")))
print(f"{nums} {words!r} {'q'!r} {e} {e!r} {1.0} {True!r}")
nested: list[list[int]] = [[1], []]
maybe: dict[str, list[bool]] = {"a": [True, False]}
print(nested, maybe, [nested, nested])
s = "text: " + str(nums)
print(s, len(str(nums)))
