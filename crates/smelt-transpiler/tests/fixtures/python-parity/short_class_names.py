# Classes named like the type parameters of the runtime's tuple impls (`A`
# to `F`) and like Rust prelude types; a class hierarchy four levels deep.
import json


class A:
    def __init__(self, x: int) -> None:
        self.x = x

    def get(self) -> int:
        return self.x

    def kind(self) -> str:
        # Inherited by `D`, four levels down: the body is re-emitted into
        # each class, `self` included.
        return type(self).__name__ + "!"


class B(A):
    def __repr__(self) -> str:
        return f"B({self.x})"


class C(B):
    pass


class D(C):
    def get(self) -> int:
        return self.x + 1


class E:
    pass


class F:
    def __init__(self, t: tuple[int, str]) -> None:
        self.t = t


class Box:
    def __init__(self, v: int) -> None:
        self.v = v


class Result:
    def __init__(self, ok: bool) -> None:
        self.ok = ok


xs: list[A] = [B(1), D(2), C(3)]
for a in xs:
    print(type(a).__name__, isinstance(a, D), isinstance(a, B), a.get(), a.kind())
print(D(9).kind(), C(9).kind())
pairs: list[tuple[int, str]] = [(2, "b"), (1, "a")]
print(sorted(pairs), repr((1, "x", 2.5, True, None, "y")), F((1, "a")).t)
print(json.dumps((1, "a", [2, 3])), json.dumps({"k": (1, 2)}))
print([B(5), B(6)], (B(7), 8), type(E()).__name__, Box(3).v, Result(True).ok)
