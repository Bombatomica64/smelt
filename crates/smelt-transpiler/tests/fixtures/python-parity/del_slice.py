xs = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
alias = xs
del xs[2:4]
print(xs, alias is xs)
del xs[-2:]
print(xs)
del xs[:1]
print(xs)
del xs[::2]
print(xs)
ys = [1, 2, 3]
del ys[5:10]
print(ys)
del ys[:]
print(ys, len(ys))
zs = list(range(12))
del zs[1:9:3]
print(zs)
del zs[::-3]
print(zs)


def drop_middle(values: list[int]) -> None:
    del values[1:-1]


ws = [5, 6, 7, 8]
drop_middle(ws)
print(ws)
try:
    del ws[::0]
except ValueError as error:
    print("ValueError:", error)
names = ["a", "b", "c", "d"]
lo = 1
del names[lo:lo + 2]
print(names)
