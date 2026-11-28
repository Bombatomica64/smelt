"""`super()` calls chained through every level of a class hierarchy.

A body that calls `super().f()` runs on instances of every class that
inherits it, and each time reaches the `f` that follows the body's own class
in the instance's MRO.
"""


class Animal:
    def __init__(self, name: str) -> None:
        self.name = name

    def describe(self) -> str:
        return f"animal {self.name}"

    def sound(self) -> str:
        return "..."

    def __str__(self) -> str:
        return f"<{self.describe()}>"


class Dog(Animal):
    def describe(self) -> str:
        return "dog > " + super().describe()

    def sound(self) -> str:
        return "woof " + super().sound()


class Puppy(Dog):
    def describe(self) -> str:
        return "puppy > " + super().describe()


class TinyPuppy(Puppy):
    def sound(self) -> str:
        return "yip " + super().sound()

    def __str__(self) -> str:
        return "tiny " + super().__str__()


class Counter:
    def __init__(self) -> None:
        self.calls: list[str] = []

    def step(self, n: int) -> int:
        self.calls.append(f"Counter.step({n})")
        return n


class Doubling(Counter):
    def step(self, n: int) -> int:
        self.calls.append(f"Doubling.step({n})")
        return super().step(n * 2)


class Logging(Doubling):
    def step(self, n: int) -> int:
        self.calls.append(f"Logging.step({n})")
        return super().step(n) + 1


animals: list[Animal] = [Animal("a"), Dog("d"), Puppy("p"), TinyPuppy("t")]
for animal in animals:
    print(animal.describe(), "|", animal.sound(), "|", animal)

logger = Logging()
print(logger.step(5))
print(logger.calls)
doubler = Doubling()
print(doubler.step(1), doubler.calls)
