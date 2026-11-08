class Point:
    x: float
    y: float

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y

    def norm2(self) -> float:
        return self.x * self.x + self.y * self.y
