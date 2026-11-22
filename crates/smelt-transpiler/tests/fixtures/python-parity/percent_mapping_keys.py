def describe(info: dict[str, str]) -> str:
    return "%(name)s works in %(city)-8s|" % info


def counter() -> int:
    print("evaluated once")
    return 7


print("%(name)s has %(n)d items, %(ratio).1f%% done, %(n)03d" % {"name": "Ada", "n": 3, "ratio": 0.25})
d = {"name": "Ada", "n": 3}
print("%(name)s has %(n)s, %(n)r" % d)
counts = {"a": 1, "b": 22}
print("%(a)d/%(b)x" % counts)
print("%(name)s!" % {"name": "lit"})
print("%(k)r and %(k)s" % {"k": "quoted"})
print("%(v)s %(v)s" % {"v": counter(), "v": 9})
print("%(x)5.2f|%(x)-6d|%(x)+d" % {"x": 3.14159})
print(describe({"name": "Grace", "city": "NYC"}))
try:
    print(describe({"name": "Bob"}))
except KeyError as e:
    print("KeyError", e)
print("%(a(b))s" % {"a(b)": "nested"})
