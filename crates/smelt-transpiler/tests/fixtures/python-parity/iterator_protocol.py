# A class with `__iter__`/`__next__` drives `for`: `__iter__` may return
# another object, `StopIteration` from `__next__` ends the loop, and `break`
# and `continue` act on it.
class Countdown:
    def __init__(self, start: int) -> None:
        self.current = start

    def __iter__(self) -> "Countdown":
        return self

    def __next__(self) -> int:
        if self.current <= 0:
            raise StopIteration
        self.current -= 1
        return self.current + 1


class WordCursor:
    def __init__(self, words: list[str]) -> None:
        self.words = words
        self.at = 0

    def __next__(self) -> str:
        if self.at >= len(self.words):
            raise StopIteration
        self.at += 1
        return self.words[self.at - 1]


class Words:
    def __init__(self, text: str) -> None:
        self.text = text

    def __iter__(self) -> "WordCursor":
        return WordCursor(self.text.split(" "))


class NullFile:
    def __iter__(self) -> "NullFile":
        return self

    def __next__(self) -> str:
        raise StopIteration


total = 0
for value in Countdown(4):
    if value == 3:
        continue
    if value == 1:
        break
    total += value
print(total)
for word in Words("to be or not"):
    print(word)
lines = 0
for line in NullFile():
    lines += 1
print(lines)
