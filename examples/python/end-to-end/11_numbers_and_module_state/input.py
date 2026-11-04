class Counter:
    def __init__(self, start: int) -> None:
        print("made counter")
        self.value = start

    def tick(self) -> int:
        self.value = self.value + 2
        return self.value


print("before")
counter = Counter(10)
print("after")


def bump() -> int:
    return counter.tick()


def run() -> None:
    x: float = 12.0
    print(x)
    print(3.0 * 4.0)
    print(0.1 + 0.2)
    print(1e16)
    print(0.00001)
    print(-0.0)
    big: int = 9007199254740993
    print(big)
    print("v " + str(x))
    print(f"f {x} {big} {True}")
    print(7 / 2)
    n: int = 3
    print(n + 0.5)
    print(n * 2)
    counter.tick()
    print(counter.tick())
    print(bump())
    print(counter.value)


run()
