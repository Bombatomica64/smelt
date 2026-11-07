# A user method spelled like a builtin collection method (`get`, `pop`,
# `count`, `copy`, `update`, `clear`) is the user's method, whatever its arity.
class Store:
    def __init__(self) -> None:
        self.value = 7
        self.cleared = False

    def get(self) -> int:
        return self.value

    def pop(self) -> int:
        return self.value - 1

    def count(self) -> int:
        return 3

    def clear(self, flag: bool) -> bool:
        self.cleared = flag
        return self.cleared


store = Store()
print(store.get(), store.pop(), store.count(), store.clear(True))
scores: dict[str, int] = {"a": 1}
print(scores.get("a"), scores.get("b", 0), [1, 2, 1].count(1))
