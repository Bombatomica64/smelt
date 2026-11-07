"""Lists index, print and divide the way CPython does."""


def read(values: list[int], index: int) -> str:
    try:
        return str(values[index])
    except IndexError:
        return "IndexError"


def halve(start: int) -> float:
    value = start
    print(value)
    value /= 2
    return value + 1


names: list[str] = ["a", "it's", 'say "x"', "back\\slash"]
print(names)
print([[1, 2], [3]], [True, False], [1.5, 2.0], ("a", 1))
print(read([1, 2], 1), read([1, 2], 2), read([1, 2], -1), read([1, 2], -3))
grid: list[list[int]] = [[1], [2, 3]]
grid[1][0] = 9
grid[1][1] += 1
print(grid, grid[-1][-1])
x = 7
print(x)
x /= 2
print(x)
print(halve(5))
