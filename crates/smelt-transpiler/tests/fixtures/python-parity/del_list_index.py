def drop_first(items: list[int]) -> None:
    del items[0]


xs = [1, 2, 3, 4, 5]
del xs[1]
print(xs)
del xs[-1]
print(xs)
alias = xs
drop_first(xs)
print(xs, alias)
for bad in [3, -4, 7]:
    try:
        del xs[bad]
    except IndexError as e:
        print("IndexError:", e)
empty: list[str] = []
try:
    del empty[0]
except IndexError as e:
    print("IndexError:", e)
grid = [[1, 2], [3, 4]]
del grid[0][1]
del grid[-1]
print(grid)
i = 0
del xs[i - 2]
print(xs)
