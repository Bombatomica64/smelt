def add(a: int, b: int) -> int:
    return a + b


def greet(name: str) -> str:
    return "hello " + name


def scale(xs: list[float], k: float) -> list[float]:
    return [x * k for x in xs]


def pad(s: str, width: int = 5) -> str:
    return s + "." * (width - len(s))
