def neg(x: int) -> int:
    return -x


def boom(x: int) -> int:
    if x == 2:
        raise ValueError("bad item")
    return x


calls: list[int] = []


def traced(x: int) -> int:
    calls.append(x)
    return x % 3


def square(v: int) -> int:
    return v * v


def size_then_text(w: str) -> tuple[int, str]:
    return (len(w), w)


def negated(f: float) -> float:
    return -f


xs = [3, -7, 2, 7, -3]
print(min(xs, key=square), max(xs, key=square))
print(min(xs, key=neg), max(xs, key=neg))
print(min(xs, key=traced), max(xs, key=traced), calls)
words = ["bb", "a", "ccc", "dd"]
print(max(words, key=size_then_text), min(words, key=None))
fs = [2.5, float("nan"), 1.0]
print(min(fs, key=lambda f: f), max(fs, key=negated))
empty: list[int] = []
try:
    print(min(empty, key=neg))
except ValueError as error:
    print("ValueError", error)
try:
    print(max(xs, key=boom))
except ValueError as error:
    print("ValueError", error)
print(max({4, 1, 9}, key=neg))
words = ["bb", "a", "ccc", "dd"]
print(max(words, key=len), min(words, key=len))
print(sorted(words, key=len), sorted(words, key=len, reverse=True))
print(sorted([3, -7, 2], key=abs), min([3, -7, 2], key=abs), max([3, -7, 2], key=abs))
print(list(map(str, [1, 2, 3])), list(map(abs, [-1, 2])))
print(sorted(["10", "9", "100"], key=int))
nested = [[1, 2, 3], [4], [5, 6]]
print(max(nested, key=len), min(nested, key=sum))
print(sorted(nested, key=len), sorted(nested, key=sum, reverse=True))
