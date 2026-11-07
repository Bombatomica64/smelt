# A dict iterates in insertion order (guaranteed since Python 3.7): `print`,
# `keys()`, `values()`, `items()`, `for`, comprehensions and `list(d)` all see
# the keys in the order they were first inserted. Deleting a key and
# inserting it again moves it to the end; overwriting a key keeps its place.

counts: dict[str, int] = {}
for word in ["zeta", "alpha", "mid", "beta", "omega", "gamma", "delta", "eps", "alpha", "zeta"]:
    counts[word] = counts.get(word, 0) + 1
print(counts)
print(list(counts.keys()))
print(list(counts.values()))
print(list(counts.items()))
print(list(counts))
for key in counts:
    print(key, counts[key])
for pair in counts.items():
    print(pair[0], pair[1])

counts["mid"] = 40
del counts["alpha"]
counts["alpha"] = 99
print(counts)
print(counts.pop("beta"), counts.pop("nope", -1))
print(counts)
print(counts.setdefault("new", 7), counts.setdefault("zeta", 0))
counts.update({"omega": 0, "tail": 1})
print(counts)

doubled = {key: counts[key] * 2 for key in counts}
print(doubled)
evens = [key for key in counts if counts[key] % 2 == 0]
print(evens)

by_number: dict[int, str] = {5: "five", 1: "one", 3: "three"}
by_number[0] = "zero"
by_number[-2] = "minus two"
print(by_number, list(by_number))
nested: dict[str, list[int]] = {"b": [2], "a": [1, 1]}
nested["c"] = []
print(nested)
