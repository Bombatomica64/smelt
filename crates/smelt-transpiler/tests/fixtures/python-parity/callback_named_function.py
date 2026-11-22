def neg(x: int) -> int:
    return -x


def last_char(word: str) -> str:
    return word[-1]


def safe_inverse(x: float) -> float:
    if x == 0:
        raise ZeroDivisionError("zero has no inverse")
    return 1 / x


xs = [3, 1, 2]
print(sorted(xs, key=neg), sorted(xs, key=neg, reverse=True))
xs.sort(key=neg)
print(xs)
words = ["kiwi", "apple", "fig"]
print(sorted(words, key=last_char))
words.sort(key=last_char, reverse=True)
print(words)
print(sorted([4.0, 0.5, 2.0], key=safe_inverse))
try:
    print(sorted([1.0, 0.0], key=safe_inverse))
except ZeroDivisionError as e:
    print("ZeroDivisionError:", e)
print(list(map(neg, xs)), list(map(last_char, words)))
