# `str` methods with CPython's rules: title case (`ß` titles to `Ss`),
# padding, bounded count/find/index, whitespace and bounded splitting, counted
# replacement and character-set stripping, with their ValueError/TypeError.


def main() -> None:
    s = "hello wORLD they're 3rd"
    print(s.title(), s.capitalize(), s.swapcase(), "ß".title(), "ﬁx".title(), "ǆemal".capitalize())
    print("-42".zfill(6), "+7".zfill(4), "abc".zfill(2), "ab".center(7, "*"), "ab".center(6), "x".ljust(3, "-"), "x".rjust(3))
    print("aaaa".count("aa"), "abc".count(""), "abcabc".count("b", 2), "abcabc".count("b", -2), "hello".find("l", 3))
    print("hello".find("l", -1), "hello".rfind("l", 0, 3), "abc".find("", 5), "abc".find("", 3), "abcabc".index("c", 3), "abcabc".rindex("a"))
    print(" a  b \t c\n".split(), " a b c ".split(None, 1), "a,b,,c".split(",", 2), ''.split(), ''.split(","))
    print(" a b c ".rsplit(maxsplit=1), "a,b,c".rsplit(",", 1), "a b".split(sep=None), "k=v=w".split("=", maxsplit=1))
    print("aaaa".replace("a", "b", 2), "abc".replace("", "-"), "abc".replace("", "-", 2), "aaaa".replace("a", "b", count=1))
    print("xxhixx".strip("x"), "abcba".lstrip("ab"), "abcba".rstrip("ba"), "ünï".title(), "Σίσυφος".swapcase())
    try:
        print("abc".index("z"))
    except ValueError as error:
        print("ValueError:", error)
    try:
        print("ab".center(5, "xy"))
    except TypeError as error:
        print("TypeError:", error)
    try:
        print("a b".split(""))
    except ValueError as error:
        print("ValueError:", error)


main()
