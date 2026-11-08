class Point:
    x: float
    y: float
    labelText: str

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y
        self.labelText = "p"

    def norm2(self) -> float:
        return self.x * self.x + self.y * self.y

    def scaledBy(self, k: float) -> float:
        return self.x * k
