"""Multiple inheritance: C3 method resolution, cooperative `super()` and
`__init__`, one object behind every base view
(docs/design/python-multiple-inheritance.md)."""

# --- mixin ---
class Animal:
    def __init__(self, name: str) -> None:
        self.name = name

    def speak(self) -> str:
        return "..."

    def describe(self) -> str:
        return self.name + " says " + self.speak()


class LoudMixin:
    def shout(self) -> str:
        return self.speak().upper() + "!"

    def speak(self) -> str:
        return "mixin"


class Dog(Animal, LoudMixin):
    def speak(self) -> str:
        return "woof"


class Robot(LoudMixin, Animal):
    pass


def loud(m: LoudMixin) -> str:
    return m.shout()


def main_mixin() -> None:
    d = Dog("rex")
    r = Robot("r2")
    print(d.describe(), d.shout(), loud(d))
    print(r.describe(), r.shout(), loud(r))
    xs: list[Animal] = [d, r, Animal("a")]
    for x in xs:
        print(x.describe(), isinstance(x, LoudMixin))
    ms: list[LoudMixin] = [d, r]
    for m in ms:
        print(m.shout(), isinstance(m, Animal))


# --- diamond ---
class Base:
    def __init__(self) -> None:
        print("Base.__init__")
        self.log: list[str] = []
        self.hits = 0

    def who(self) -> str:
        return "Base"

    def chain(self) -> str:
        return "Base"


class Left(Base):
    def __init__(self) -> None:
        print("Left.__init__ start")
        super().__init__()
        self.left = 1
        print("Left.__init__ end")

    def who(self) -> str:
        return "Left"

    def chain(self) -> str:
        return "Left>" + super().chain()


class Right(Base):
    def __init__(self) -> None:
        print("Right.__init__ start")
        super().__init__()
        self.right = 2
        print("Right.__init__ end")

    def chain(self) -> str:
        return "Right>" + super().chain()

    def only_right(self) -> int:
        return self.right * 10


class Bottom(Left, Right):
    def __init__(self) -> None:
        print("Bottom.__init__ start")
        super().__init__()
        print("Bottom.__init__ end")

    def chain(self) -> str:
        return "Bottom>" + super().chain()


def poke_left(l: Left) -> None:
    l.hits += 1
    l.log.append("left")


def poke_right(r: Right) -> None:
    r.hits += 10
    r.log.append("right")


def same(a: Base, b: Base) -> bool:
    return a is b


def main_diamond() -> None:
    b = Bottom()
    print(b.who(), b.chain(), b.left, b.right, b.only_right())
    poke_left(b)
    poke_right(b)
    print(b.hits, b.log)
    left: Left = b
    right: Right = b
    print(same(left, right), left.hits, right.hits)
    shapes: list[Base] = [Left(), Right(), b]
    for s in shapes:
        print(s.who(), s.chain(), isinstance(s, Left), isinstance(s, Right), isinstance(s, Bottom))


# --- coop ---
class Named:
    def __init__(self, name: str) -> None:
        super().__init__()
        self.name = name

    def label(self) -> str:
        return "named " + self.name


class Counted:
    def __init__(self) -> None:
        super().__init__()
        self.count = 0

    def bump(self) -> int:
        self.count += 1
        return self.count


class Item(Named, Counted):
    def __init__(self, name: str) -> None:
        super().__init__(name)

    def __str__(self) -> str:
        return f"Item({self.name}, {self.count})"


class Reprable:
    def __repr__(self) -> str:
        return "<" + self.tag() + ">"

    def tag(self) -> str:
        return "reprable"


class Point(Reprable):
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

    def tag(self) -> str:
        return f"{self.x},{self.y}"


class Tagged(Point, Counted):
    def __init__(self, x: int, y: int) -> None:
        Point.__init__(self, x, y)
        Counted.__init__(self)

    def tag(self) -> str:
        return "tagged " + super().tag()


def show(n: Named) -> str:
    return n.label()


def tick(c: Counted) -> int:
    return c.bump()


def main_coop() -> None:
    it = Item("x")
    tick(it)
    tick(it)
    print(show(it), it.count, it)
    plain = Named("solo")
    print(plain.label(), isinstance(plain, Counted))
    t = Tagged(1, 2)
    tick(t)
    print(t, repr(t), t.count, [t, Point(3, 4)])
    things: list[Counted] = [it, t, Counted()]
    print([tick(c) for c in things])


# --- harder ---
class Shape:
    sides = 0

    def __init__(self, name: str) -> None:
        self.name = name

    @property
    def title(self) -> str:
        return self.name.upper()

    @classmethod
    def kind(cls) -> str:
        return "shape"

    def area(self) -> float:
        return 0.0


class Colored:
    def __init__(self) -> None:
        self.color = "red"

    def paint(self, color: str) -> None:
        self.color = color

    def describe(self) -> str:
        return self.color + " thing"


class Square(Shape, Colored):
    def __init__(self, side: float) -> None:
        Shape.__init__(self, "square")
        Colored.__init__(self)
        self.side = side

    def area(self) -> float:
        return self.side * self.side

    def describe(self) -> str:
        return self.title + " " + super().describe()


class Root:
    def hello(self) -> str:
        return "Root"


class A(Root):
    def hello(self) -> str:
        return "A>" + super().hello()


class B(Root):
    def hello(self) -> str:
        return "B>" + super().hello()


class C(A, B):
    def hello(self) -> str:
        return "C>" + super().hello()


class D(B, A):
    pass


class E(C):
    def hello(self) -> str:
        return "E>" + super().hello()


def total(shapes: list[Shape]) -> float:
    return sum(s.area() for s in shapes)


def paint_all(items: list[Colored], color: str) -> None:
    for item in items:
        item.paint(color)


def main_harder() -> None:
    sq = Square(3.0)
    print(sq.title, sq.area(), sq.describe(), sq.color)
    paint_all([sq], "blue")
    print(sq.describe(), total([sq, Shape("blob")]))
    everything: list[Root] = [C(), D(), E(), A(), B()]
    for x in everything:
        print(type(x).__name__, x.hello())
    bases: list[Root] = [C(), D(), E()]
    print([b.hello() for b in bases])
    print(isinstance(E(), B), isinstance(D(), C), isinstance(sq, Colored))


# --- raising ---
class Validator:
    def check(self, value: int) -> int:
        if value < 0:
            raise ValueError(f"negative: {value}")
        return value


class Store:
    def __init__(self) -> None:
        self.items: list[int] = []

    def check(self, value: int) -> int:
        return value

    def add(self, value: int) -> None:
        self.items.append(self.check(value))


class SafeStore(Validator, Store):
    def __init__(self) -> None:
        super().__init__()


def fill(store: Store, values: list[int]) -> str:
    try:
        for v in values:
            store.add(v)
        return "ok"
    except ValueError as e:
        return "rejected " + str(e)


def main_raising() -> None:
    s = SafeStore()
    print(fill(s, [1, 2, -3, 4]), s.items)
    print(fill(Store(), [1, -2]))


main_mixin()
main_diamond()
main_coop()
main_harder()
main_raising()
