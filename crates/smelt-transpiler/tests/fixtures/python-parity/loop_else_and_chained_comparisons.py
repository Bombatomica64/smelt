# A loop's `else` runs only when the loop ends without `break`; a chained
# comparison `a < b < c` evaluates `b` once and stops at the first false link;
# `and`/`or` evaluate their right operand only when the left does not decide.
def find_first_even(xs: list[int]) -> str:
    for x in xs:
        if x % 2 == 0:
            result = f"found {x}"
            break
    else:
        result = "none"
    return result


def countdown(n: int) -> int:
    steps = 0
    while n > 0:
        n -= 1
        steps += 1
        if steps > 100:
            break
    else:
        steps += 1000
    return steps


def tick(log: list[str], name: str, value: int) -> int:
    log.append(name)
    return value


def check(log: list[str], name: str, value: bool) -> bool:
    log.append(name)
    return value


def main() -> None:
    calls: list[str] = []
    print(check(calls, "a", False) and check(calls, "b", True), check(calls, "c", True) or check(calls, "d", True), calls)
    print(check(calls, "e", True) and check(calls, "f", False) or check(calls, "g", True), calls)
    a, b, c = 1, 5, 9
    print(a < b < c, a < c < b, 0 <= a <= 1 == 1, 1 < 2 > 0)
    log: list[str] = []
    print(tick(log, "x", 3) < tick(log, "y", 1) < tick(log, "z", 2), log)
    log2: list[str] = []
    print(tick(log2, "x", 1) < tick(log2, "y", 2) < tick(log2, "z", 3), log2)
    print(find_first_even([1, 3, 4, 5]), find_first_even([1, 3]), countdown(3))
    for i in range(3):
        for j in range(3):
            if j == 1:
                break
        else:
            print("inner else never runs")
    else:
        print("outer else runs", i)


main()
