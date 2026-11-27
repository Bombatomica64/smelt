class Counter:
    count = 0
    label = "counter"
    ratio = 1.5
    enabled = True

    def __init__(self) -> None:
        Counter.count += 1
        self.serial = Counter.count

    def describe(self) -> str:
        return self.label + "#" + str(self.serial) + " of " + str(self.count)

    @classmethod
    def total(cls) -> int:
        return cls.count

    @classmethod
    def bump(cls, by: int) -> None:
        cls.count += by


class Named:
    name = "default"
    hits = 0

    def __init__(self, given: str) -> None:
        self.name = given.upper()

    def greet(self) -> str:
        Named.hits = Named.hits + 1
        return "hi " + self.name + " (class " + Named.name + ")"


class Base:
    kind = "base"
    created = 0

    def __init__(self) -> None:
        Base.created += 1


class Child(Base):
    pass


def main() -> None:
    a = Counter()
    b = Counter()
    print(Counter.count, a.count, b.count)
    print(a.describe())
    print(b.describe())
    Counter.bump(10)
    print(Counter.total(), a.count)
    Counter.ratio *= 2
    print(Counter.ratio, a.ratio)
    Counter.label = "ctr"
    print(a.describe())
    print(Counter.enabled)
    n = Named("ada")
    print(n.greet())
    print(n.greet())
    print(Named.hits, Named.name, n.name)
    c = Child()
    Base()
    print(Child.kind, c.kind, Child.created, Base.created)


main()
