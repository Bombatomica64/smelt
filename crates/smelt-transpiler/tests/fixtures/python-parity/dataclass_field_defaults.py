from dataclasses import dataclass, field


def fresh_scores() -> dict[str, int]:
    print("factory runs")
    return {"start": 0}


@dataclass
class Pt:
    x: int
    y: int = 0
    tags: list[str] = field(default_factory=list)
    scores: dict[str, int] = field(default_factory=fresh_scores)
    label: str = field(default="pt")


def main() -> None:
    p = Pt(1)
    q = Pt(1)
    print(p, p == q, Pt(2, 3).y)
    p.tags.append("t")
    print(q.tags, p.tags, p.scores, q.label)
    r = Pt(5, 6, ["a"], {"k": 1}, "r")
    print(r)



@dataclass
class Bag:
    owner: str
    items: list[int] = field(default_factory=list)


def equality() -> None:
    a = Bag("a")
    b = Bag("a")
    print(a == b, a != b)
    a.items.append(1)
    print(a == b, a == a)
    b.items.append(1)
    print(a == b, Bag("x", [2]) == Bag("y", [2]))


main()
equality()
