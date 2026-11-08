def add(a: float, b: float) -> float:
    return a + b


def greet(name: str) -> str:
    return "hello " + name


def scale(xs: list[float], k: float) -> list[float]:
    return [x * k for x in xs]
