def tick(n: int) -> int:
    print("tick", n)
    return n


def main() -> None:
    print("{} {:>5} {:<3}|".format(1, "r", "l"))
    print("{0}-{1}-{0}".format("a", "b"))
    print("{name} is {age:03d}".format(name="Ann", age=7))
    print("{!r} {!s} {:.3f} {:,}".format("q", "s", 3.14159, 1234567))
    print("{{literal}} {}".format([1, 2]))
    print("{1}{0}".format(tick(1), tick(2)))
    print("pi={:^9.2f}|".format(3.14159))
    print("{}".format(True), "{:>4}".format(True))



def radix() -> None:
    n = 255
    print(hex(n), hex(-255), bin(5), bin(0), oct(8), oct(-9), hex(10**20))
    print(format(3.14159, ".2f"), format(42, ">6"), format(7), format("s", "^5"), format(True))
    words = ["b", "A", "c", "B"]
    print(sorted(words, key=str.lower), list(map(str.upper, words)), max(words, key=str.lower))


main()
radix()
