"""Python's `collections` containers, typed end to end: word counts with
`Counter`, grouping with `defaultdict(list)`, a breadth-first search over a
`deque`, a bounded history, and an LRU cache on `OrderedDict`."""

import collections
from collections import Counter, OrderedDict, defaultdict, deque
from typing import NamedTuple


def word_counts(text: str) -> Counter[str]:
    counts: Counter[str] = Counter(text.split())
    return counts


def group_by_length(words: list[str]) -> defaultdict[int, list[str]]:
    groups: defaultdict[int, list[str]] = defaultdict(list)
    for word in words:
        groups[len(word)].append(word)
    return groups


def bfs(graph: dict[str, list[str]], start: str) -> list[str]:
    order: list[str] = []
    seen: set[str] = {start}
    queue: deque[str] = deque([start])
    while queue:
        node = queue.popleft()
        order.append(node)
        for nxt in graph[node]:
            if nxt not in seen:
                seen.add(nxt)
                queue.append(nxt)
    return order


class LruCache:
    def __init__(self, capacity: int) -> None:
        self.capacity = capacity
        self.entries: OrderedDict[str, int] = OrderedDict()

    def get(self, key: str) -> int:
        if key not in self.entries:
            return -1
        self.entries.move_to_end(key)
        return self.entries[key]

    def put(self, key: str, value: int) -> None:
        self.entries[key] = value
        self.entries.move_to_end(key)
        if len(self.entries) > self.capacity:
            self.entries.popitem(last=False)


class Point(NamedTuple):
    x: int
    y: int
    label: str = "origin"


class Pair(NamedTuple):
    left: int
    right: int


def named_tuple_section() -> None:
    p = Point(1, 2, "a")
    q = Point(x=3, y=4)
    print(p, q)
    print(p.x + q.y, p[0], p[-1], q[2], len(p))
    x, y, label = p
    print(x, y, label)
    moved = p._replace(y=20, label="moved")
    print(moved, p)
    pair = Pair(5, 6)
    print(pair._asdict(), pair)
    print(p == Point(1, 2, "a"), p == moved)
    path: deque[Point] = deque([p, q])
    path.rotate(1)
    print(path)


def counter_section() -> None:
    counts = word_counts("the cat and the hat and the bat")
    print(counts)
    print(counts["the"], counts["dog"], "dog" in counts)
    print(counts.most_common(2))
    print(counts.most_common())
    print(counts.total(), len(counts))
    letters = Counter("abracadabra")
    print(letters)
    print(sorted(letters.elements()))
    letters.update("aaz")
    letters["b"] += 10
    print(letters.most_common(3))
    letters.subtract("zz")
    print(letters["z"], letters)
    other: Counter[str] = Counter({"a": 1, "q": 4})
    print(letters + other)
    print(letters - other)
    print(Counter("ab") == Counter("ba"), Counter("ab") != Counter("abb"))
    del letters["q"]
    del letters["a"]
    print(letters)
    empty: Counter[int] = Counter()
    print(empty, bool(empty), bool(letters))
    for key in Counter([3, 1, 3, 2, 1, 3]):
        print("key", key)
    print(list(Counter("hello").items()))


def defaultdict_section() -> None:
    groups = group_by_length(["a", "bb", "cc", "ddd", "e"])
    print(groups)
    print(groups[2], groups.get(9), 9 in groups)
    print(groups[7], 7 in groups, len(groups))
    tally: defaultdict[str, int] = defaultdict(int)
    for ch in "mississippi":
        tally[ch] += 1
    print(tally)
    print(sorted(tally.items()))
    plain: dict[str, int] = dict(tally)
    print(plain, dict(OrderedDict([("k", 1)])))
    print(list(tally.keys()), list(tally.values()))
    nested: defaultdict[str, set[int]] = defaultdict(set)
    nested["x"].add(1)
    nested["x"].add(1)
    nested["y"].add(2)
    print(len(nested["x"]), len(nested["y"]), len(nested))
    scores: defaultdict[str, float] = defaultdict(lambda: 1.5)
    scores["a"] += 1.0
    print(scores["a"], scores["b"], scores.pop("a"), len(scores))
    for key in tally:
        if tally[key] > 2:
            print("frequent", key)
    labels: defaultdict[str, str] = collections.defaultdict(str, {"k": "v"})
    print(labels)
    print(labels["missing"] == "")
    print(labels)


def deque_section() -> None:
    graph = {"a": ["b", "c"], "b": ["d"], "c": ["d", "e"], "d": ["f"], "e": ["f"], "f": []}
    print(bfs(graph, "a"))
    window: deque[int] = deque(maxlen=3)
    for value in range(6):
        window.append(value)
        print(list(window), sum(window))
    print(window, window.maxlen)
    items = deque([1, 2, 3, 4, 5])
    items.rotate(2)
    print(items)
    items.rotate(-3)
    print(items)
    items.appendleft(0)
    items.extend([6, 7])
    items.extendleft([-2, -1])
    print(items, len(items), items[0], items[-1])
    items[1] = 100
    print(items.pop(), items.popleft(), items)
    print(items.count(3), items.index(3), 3 in items, 99 in items)
    items.remove(100)
    items.reverse()
    print(items)
    copied = items.copy()
    copied.append(42)
    print(items == copied, len(items), len(copied))
    try:
        deque([1]).index(5)
    except ValueError as err:
        print("ValueError:", err)
    empty: deque[str] = deque()
    try:
        empty.pop()
    except IndexError as err:
        print("IndexError:", err)
    print(empty, bool(empty), [x * 2 for x in deque([1, 2, 3])])


def ordered_dict_section() -> None:
    cache = LruCache(2)
    cache.put("a", 1)
    cache.put("b", 2)
    print(cache.get("a"))
    cache.put("c", 3)
    print(cache.get("b"), cache.get("c"), cache.entries)
    ordered = OrderedDict([("x", 1), ("y", 2), ("z", 3)])
    ordered.move_to_end("x")
    print(ordered)
    ordered.move_to_end("z", last=False)
    print(list(ordered.keys()), ordered.popitem(), ordered.popitem(last=False))
    print(ordered, len(ordered), ordered.get("y"), ordered.get("q", 0))
    try:
        print(ordered["missing"])
    except KeyError as err:
        print("KeyError:", err)
    empty: OrderedDict[str, int] = OrderedDict()
    print(empty, OrderedDict({"b": 1, "a": 2}) == OrderedDict({"a": 2, "b": 1}))


named_tuple_section()
counter_section()
defaultdict_section()
deque_section()
ordered_dict_section()
