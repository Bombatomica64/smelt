def pick(n: int) -> int:
    print("pick", n)
    return n


def main() -> None:
    print(1 and 2, 0 and 2, "a" and "b")
    x = [1] and [2]
    print(x)
    print(pick(0) or pick(3) or pick(4))
    print(pick(2) and pick(0) and pick(9))
    a: list[int] = []
    print(a or [5])
    print(0 or 5, "" or "z")
    y: int | None = None
    print(y or 3)


main()
