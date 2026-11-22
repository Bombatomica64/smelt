import json

data: dict[str, list[int]] = {"a": [1, 2], "b": [], "c": [3]}
print(json.dumps(data, indent=2))
print(json.dumps([[1], [2, 3], []], indent=4))
print(json.dumps({"k": "x, y: [z]"}, indent=0))
print(json.dumps([[1.5, 2.5], []], indent="\t"))
print(json.dumps({"n": [1]}, indent=-3))
print(json.dumps([1, 2], indent=None))
width = 3
print(json.dumps({"deep": {"er": [[1], [2, 3]]}}, indent=width))
empty_d: dict[str, int] = {}
empty_l: list[int] = []
print(json.dumps(empty_d, indent=2), json.dumps(empty_l, indent=2))

big: int = json.loads("12345678901234567890123")
print(big, big + 1)
xs: list[int] = json.loads("[1, -2, 99999999999999999999]")
print(xs)
d: dict[str, int] = json.loads('{"n": 18446744073709551617, "m": -5, "n": 7}')
print(d)
fs: list[float] = json.loads(" [1.5, 2e3, -0.25, 1E400, -Infinity, NaN] ")
print(fs)
nested: dict[str, list[str]] = json.loads('{"a": ["x\\u00e9\\ud83d\\ude00", "q\\"\\n"]}')
print(nested)
maybe: list[int | None] = json.loads("[1, null, 3]")
print(maybe)
flag: bool = json.loads("true")
print(flag)

for bad in ["", "[1, 2", "[1 2]", '{"a" 1}', '{"a": 1,}', "[1,]", "01", '{1: 2}', '"abc', "[1] x", "\n\n  [nope]", '"a\\qb"']:
    try:
        v: list[int] = json.loads(bad)
        print("parsed", v)
    except ValueError as error:
        print("ValueError:", error)
