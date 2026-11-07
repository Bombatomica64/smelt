# `print` of a container writes `str(container)`: each element's `repr`, so
# strings are quoted, booleans are `True`/`False`, absent values `None`.
from typing import Optional

words: list[str] = ["banana", "kiwi", "apple"]
print(sorted(words, key=lambda w: len(w)))
print(sorted(words, key=lambda w: w[-1], reverse=True))
print([True, False], [1.5, 2.0, 1e16], [], (7,), (1, "a", False))
print({"a": 1}, {"x": [1, 2]}, {3: "three"})
print(["it's", 'say "hi"', "tab\there", "back\\slash", "é"])
maybe: list[Optional[int]] = [1, None, 3]
print(maybe, [[1, 2], [3]], ("nested", [True]))
print("plain", 3, 2.5, True, None)
