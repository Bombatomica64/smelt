# A closure reads a variable when it is called (late binding): closures made
# in a loop all see the loop variable's last value. A defaulted lambda
# parameter (`lambda x, i=i:`) freezes the value the loop had when the lambda
# was made.
from typing import Callable

frozen: list[Callable[[int], int]] = []
late: list[Callable[[int], int]] = []
for i in range(3):
    frozen.append(lambda x, i=i: x + i)
    late.append(lambda x: x + i)
print([f(10) for f in frozen], [f(10) for f in late])

defs: list[Callable[[int], int]] = []
for j in range(3):
    def add(x: int) -> int:
        return x + j
    defs.append(add)
print([f(100) for f in defs])


def make_scalers(factors: list[int]) -> list[Callable[[int], int]]:
    out: list[Callable[[int], int]] = []
    for factor in factors:
        out.append(lambda x, factor=factor, offset=1: x * factor + offset)
    return out


print([scale(3) for scale in make_scalers([1, 2, 5])])
base = 10
shifted: Callable[[int], int] = lambda x, b=base: x + b
unshifted: Callable[[int], int] = lambda x: x + base
base = 20
print(shifted(1), unshifted(1))
