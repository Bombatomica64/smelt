"""`list.pop()` on an empty list raises `IndexError` for every element type."""


def pop_int(values: list[int]) -> str:
    try:
        return str(values.pop())
    except IndexError as error:
        return "IndexError: " + str(error)


def pop_str(values: list[str]) -> str:
    try:
        return values.pop()
    except IndexError:
        return "IndexError"


def pop_nested(values: list[list[int]]) -> str:
    try:
        return str(values.pop())
    except IndexError:
        return "IndexError"


def drain(values: list[int]) -> int:
    total: int = 0
    while True:
        try:
            total = total + values.pop()
        except IndexError:
            return total


print(pop_int([]), pop_int([4, 5]))
print(pop_str([]), pop_str(["a"]))
print(pop_nested([]), pop_nested([[1, 2]]))
print(drain([1, 2, 3]))
