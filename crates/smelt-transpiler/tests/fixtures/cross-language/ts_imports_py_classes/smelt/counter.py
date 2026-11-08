class Counter:
    count: int
    stepSize: int

    def __init__(self, start: int) -> None:
        self.count = start
        self.stepSize = 1

    def inc(self) -> int:
        self.count += self.stepSize
        return self.count

    def bumpBy(self, n: int) -> int:
        self.count += n
        return self.count
