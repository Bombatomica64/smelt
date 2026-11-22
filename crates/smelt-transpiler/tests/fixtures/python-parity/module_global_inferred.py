def scale_of(text: str) -> float:
    return float(text) * 2


x = float("2.5")
label = "v" + str(3)
items = list(range(3))
scale = scale_of("0.5")


def f() -> float:
    return x * 2


def describe() -> str:
    return label + ":" + str(len(items)) + ":" + str(scale)


def bump() -> None:
    global x
    x = x + 1.0


print(f(), describe())
bump()
print(f(), x)
