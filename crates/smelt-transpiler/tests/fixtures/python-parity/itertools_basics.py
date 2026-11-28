# `itertools` as CPython runs it: every adapter is lazy (an infinite
# `count`/`cycle`/`repeat` ends under `islice`, `zip` or a `break`), pulls its
# source only as far as it must (`islice` leaves the rest of a shared
# iterator to the next consumer), and keeps its element types: `product`,
# `permutations` and `combinations` yield typed tuples, `groupby` yields
# `(key, group)` pairs whose groups are invalidated once the outer advances.
import itertools
from itertools import (
    accumulate,
    chain,
    combinations,
    count,
    cycle,
    groupby,
    islice,
    permutations,
    product,
    repeat,
)
from typing import Iterator


def numbers(limit: int) -> Iterator[int]:
    n = 0
    while n < limit:
        print("  produce", n)
        yield n
        n += 1


def multiply(total: int, value: int) -> int:
    return total * value


# chain / chain.from_iterable
print(list(chain([1, 2], [3], [])))
print(list(itertools.chain("ab", "cd")))
print(list(chain.from_iterable([[1, 2], [], [3, 4, 5]])))
print(sum(chain(numbers(2), [10, 20])))
letters = ["x", "y"]
print([s.upper() for s in chain(letters, ["z"])])

# islice: stop, start/stop, step, None stop, and how far it pulls
print(list(islice([10, 20, 30, 40, 50], 3)))
print(list(islice("abcdefg", 2, None)))
print(list(itertools.islice(range(20), 1, 12, 3)))
shared = numbers(6)
print(list(islice(shared, 1, 3)))
print("next after islice:", next(shared))
print(list(islice(shared, 0)))
print("still there:", next(shared))

# count: infinite, consumed lazily
print(list(islice(count(), 5)))
print(list(islice(count(10, 5), 4)))
print(list(zip(count(1), ["a", "b", "c"])))
for n in count(3, -1):
    if n < 0:
        break
    print("countdown", n)
print(next(count(7)), next(count(-2, 2)))

# repeat: bounded and infinite
print(list(repeat("ab", 3)))
print(list(repeat(1.5, 0)), list(repeat(4, -2)))
print(list(zip(repeat(0), [7, 8])))
print([base * k for base, k in zip(range(4), repeat(10))])

# cycle
print(list(islice(cycle("abc"), 7)))
print(list(islice(cycle([1, 2]), 5)))
no_players: list[str] = []
print(list(cycle(no_players)))
turns = 0
for player in cycle(["ann", "bob"]):
    turns += 1
    if turns > 3:
        break
    print("turn", turns, player)

# accumulate: running sum, binary function, initial=
print(list(accumulate([1, 2, 3, 4])))
print(list(accumulate([3, 1, 4, 1, 5], max)))
print(list(accumulate([1, 2, 3, 4], multiply)))
print(list(accumulate([1, 2, 3], initial=100)))
print(list(accumulate([], initial=5)))
print(list(accumulate(["a", "b", "c"])))
print(list(accumulate([2.5, 0.5], lambda acc, x: acc * x)))
print(list(islice(accumulate(count(1)), 5)))

# product
print(list(product([1, 2], ["a", "b"])))
print(list(product("ab", repeat=2)))
print(len(list(product(range(3), range(4), [True]))))
for x, y in product(range(2), [0.5]):
    print("cell", x, y)

# permutations / combinations
print(list(permutations([1, 2, 3])))
print(list(permutations("abc", 2)))
print(list(permutations([1, 2], 3)))
print(len(list(permutations((1, 2, 3, 4)))))
print(list(combinations([1, 2, 3, 4], 2)))
print(list(combinations("abcd", 3)))
print(list(combinations([1], 0)), list(combinations([1, 2], 5)))
pairs = [a + b for a, b in combinations([10, 20, 30], 2)]
print(pairs)

# groupby: with and without key=, groups consumed in order
for key, group in groupby("aaabccdd"):
    print(key, list(group))
words = ["apple", "avocado", "banana", "blueberry", "cherry", "apricot"]
by_letter = {k: list(g) for k, g in groupby(words, key=lambda w: w[0])}
print(by_letter)
lengths = [(k, len(list(g))) for k, g in groupby([1, 1, 2, 3, 3, 3], key=lambda n: n % 2 == 1)]
print(lengths)
runs = groupby([5, 5, 6, 6, 6, 7])
first_key, first_group = next(runs)
print(first_key, next(first_group))
second_key, second_group = next(runs)
print(second_key, list(first_group), list(second_group))
print([(k, list(g)) for k, g in list(groupby("aabbb"))])
print([k for k, _ in groupby(numbers(3), key=lambda n: n // 2)])

# pairwise, takewhile, dropwhile, filterfalse, starmap
print(list(itertools.pairwise([1, 4, 9, 16])))
print(list(itertools.pairwise("a")))
print(list(itertools.takewhile(lambda n: n < 3, count())))
print(list(itertools.dropwhile(lambda n: n < 3, [1, 5, 2, 7])))
print(list(itertools.filterfalse(lambda n: n % 2 == 0, range(7))))
print(list(itertools.filterfalse(None, [0, 3, 0, 4])))
print(list(itertools.starmap(multiply, [(2, 3), (4, 5)])))
print(list(itertools.starmap(lambda a, b: a + b, zip("ab", "cd"))))
