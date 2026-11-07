# `for k, v in d.items()` and nested tuple targets, tuple unpacking, and
# `dict.pop`/`popitem` with the `KeyError`s CPython raises.
d: dict[str, int] = {"b": 2, "a": 1, "c": 3}
for k, v in d.items():
    print(k, v)
pairs: list[tuple[str, int]] = [("x", 1), ("y", 2)]
for name, (value) in pairs:
    print(name, value)
for i, (k, v) in enumerate(d.items()):
    print(i, k, v)
a, b = 1, "two"
print(a, b)
print(d.popitem(), d)
try:
    d.pop("zz")
except KeyError as e:
    print("pop missing", e, e.args)
e2: dict[str, int] = {}
try:
    e2.popitem()
except KeyError as e:
    print("popitem empty", e)
print(d.pop("a"), d.pop("q", 0), d)
