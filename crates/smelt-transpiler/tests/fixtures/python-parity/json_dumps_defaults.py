# `json.dumps` with its default arguments: `", "` and `": "` separators, a
# float keeps its repr, ensure_ascii escapes, non-finite floats. The calls run
# from a function a module-level statement calls, which must print once.
import json
def run():
    print("hello")
    print(json.dumps([1, 2, 3]))
    print(json.dumps({"a": [1.5, 2.0], "b": [0.1]}))
    print(json.dumps({"k": "é\"x\n"}))
    print(json.dumps([True, False]))
    print(json.dumps((1, 2)))
    xs: list[int] = []
    e: dict[str, int] = {}
    print(json.dumps(xs), json.dumps(e))
run()
def more():
    d: dict[str, float] = {"x": 1e16, "y": 1e-5, "z": float("nan"), "w": float("inf")}
    print(json.dumps(d))
    o: list[int | None] = [1, None]
    print(json.dumps(o), json.dumps("𝄞\x7fÿ"), json.dumps(3), json.dumps(2.5))
more()
