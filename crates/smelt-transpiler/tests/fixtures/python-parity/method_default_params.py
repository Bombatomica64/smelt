# A method or constructor parameter with a default takes it when the caller
# leaves the argument out; a mutable default is one object shared by every
# call that omits it, as CPython evaluates it once at definition time.


class Greeter:
    def __init__(self, name: str, greeting: str = "hello", times: int = 1) -> None:
        self.name = name
        self.greeting = greeting
        self.times = times

    def greet(self, punctuation: str = "!") -> str:
        return (self.greeting + " " + self.name + punctuation) * self.times

    def collect(self, item: int, into: list[int] = []) -> int:
        into.append(item)
        return len(into)


g = Greeter("ada")
print(g.greet(), g.greeting, g.times)
print(Greeter("bob", "hi").greet("?"))
print(Greeter("cy", "yo", 2).greet())
print(g.collect(1), g.collect(2), g.collect(3, [7]), g.collect(4))
