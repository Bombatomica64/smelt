nan: float = float("nan")
xs = [3.0, nan, 1.0, 2.0]
print(sorted(xs))
print(sorted([nan, 3.0, 1.0]))
print(sorted([2.0, 1.0, nan, 0.5, -1.0], reverse=True))
ys = [5.0, 1.0, nan, 4.0, 2.0]
ys.sort()
print(ys)
print(sorted([(1.0, "b"), (nan, "a"), (0.5, "c")]))
print(sorted([3.5, 1.5], key=lambda v: -v))
print(sorted(["b", "a"], key=lambda s: nan if s == "a" else 1.0))
print(min([nan, 1.0]), max([1.0, nan, 3.0]))


def noisy(v: int) -> int:
    print("key", v)
    return -v


print(sorted([3, 1, 2], key=lambda v: noisy(v)))


def picky(v: float) -> float:
    if v > 4.0:
        raise ValueError("too big")
    return v


zs = [3.0, 1.0, 5.0, 2.0]
try:
    zs.sort(key=lambda v: picky(v))
except ValueError as error:
    print("raised", error)
print(zs)
def make(n: int, seed: int, mode: int) -> list[float]:
    out: list[float] = []
    state = seed
    for i in range(n):
        state = (state * 1103515245 + 12345) % 2147483648
        r = state % 1000
        if mode == 0:
            v = float(r % 50)
        elif mode == 1:
            v = float(i) if r % 10 != 0 else float(r)
        elif mode == 2:
            v = float(n - i) if r % 7 != 0 else float(r % 13)
        else:
            v = float((i // 37) * 3 + r % 3)
        if r % 97 == 0:
            v = nan
        out.append(v)
    return out


def digest(xs: list[float]) -> int:
    acc = 0
    for i, x in enumerate(xs):
        if x != x:
            code = 1000003
        else:
            code = int(x)
        acc = (acc * 1000033 + code + i) % 1000000007
    return acc


total = 0
for n in [0, 1, 2, 5, 31, 63, 64, 65, 100, 257, 1000, 1000]:
    for mode in range(4):
        for seed in [1, 42]:
            xs = make(n, seed, mode)
            a = sorted(xs)
            b = sorted(xs, reverse=True)
            c = sorted(xs, key=lambda v: -v)
            d = list(xs)
            d.sort()
            line = f"{n} {mode} {seed} {digest(a)} {digest(b)} {digest(c)} {digest(d)}"
            print(line)
