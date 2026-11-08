class Counter:
    count: int

    def __init__(self, start: int) -> None:
        self.count = start

    def inc(self) -> int:
        self.count += 1
        return self.count
