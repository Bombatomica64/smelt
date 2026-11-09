"""String search, slicing, negative indexes, tuple access and list mutation."""


def searches() -> None:
    word: str = "Smelted"
    print(word.find("m"), word.find("z"), word.rfind("e"), word.rfind("z"))


def slices() -> None:
    values: list[int] = [1, 2, 3, 4]
    print(values[:], values[1:], values[1:3], values[-2:], values[10:], values[:-1])
    copied: list[int] = values[:]
    copied.append(5)
    print(values, copied)
    word: str = "smelting"
    print(word[:], word[1:], word[1:4], word[-3:], word[20:])
    accented: str = "héllo"
    print(accented[1:3], accented[-1])


def negative_indexes() -> None:
    values: list[int] = [1, 2, 3]
    print(values[-1], values[-3])
    word: str = "abc"
    print(word[-1], word[-3])
    try:
        print(values[3])
    except IndexError:
        print("list read raised IndexError")
    try:
        print(word[-len(values) - 1])
    except IndexError:
        print("str read raised IndexError")


def tuples() -> None:
    pair: tuple[str, int] = ("Ada", 1)
    name: str = pair[0]
    rank: int = pair[-1]
    tail: tuple[int] = pair[1:]
    print(name, rank, tail)


def mutations() -> None:
    values: list[int] = [1, 2]
    values.append(3)
    print(values)
    right: list[int] = [4, 5]
    values.extend(right)
    right.append(6)
    print(values, right)
    values.insert(1, 0)
    values.insert(-1, 9)
    values.insert(100, 7)
    print(values)
    values.reverse()
    print(values)
    print(values.pop(), values)
    mapping: dict[str, int] = {"a": 1}
    values.clear()
    mapping.clear()
    print(values, mapping)


searches()
slices()
negative_indexes()
tuples()
mutations()
