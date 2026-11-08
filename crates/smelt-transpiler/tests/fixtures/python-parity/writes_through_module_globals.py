# A write THROUGH a module global (a field, a property setter, an index)
# lands in the global itself: every later read, in the module or in a
# function, sees it. The global is a class instance or a list, not a map.


class Counter:
    count: int
    label: str

    def __init__(self, count: int) -> None:
        self.count = count
        self.label = "c"

    @property
    def doubled(self) -> int:
        return self.count * 2

    @doubled.setter
    def doubled(self, value: int) -> None:
        self.count = value // 2


counter: Counter = Counter(1)
items: list[int] = [1, 2, 3]


def bump() -> None:
    counter.count += 10
    items[0] = items[0] + 100


def describe() -> str:
    return counter.label + ":" + str(counter.count) + ":" + str(counter.doubled)


counter.count = 4
counter.label = "k"
print(describe())
counter.doubled = 30
print(describe())
bump()
print(describe(), items)
items[2] = 7
counter.count -= 1
print(counter.count, items, counter.doubled)
