# `round`, `divmod`, `pow`, `chr`, `ord` and `list.pop(i)` with CPython's
# results and exceptions: round half to even, correctly rounded `ndigits`.


def main() -> None:
    print(round(2.5), round(3.5), round(-0.5), round(-1.5), round(2.675, 2), round(1234.5678, -2), round(0.5, 0))
    print(round(1250, -2), round(1350, -2), round(-1250, -2), round(5, 2), round(1e300, 2), round(123.456, -5), round(-123.456, -5))
    print(round(float("inf"), 2), round(2 ** 70 + 51, -2), round(0.1 + 0.2, 1), round(1.0000000000000002, 15))
    print(chr(65), chr(0x1F600), ord("A"), ord("é"), divmod(-7, 2), divmod(7.5, 2), divmod(2 ** 70, 3))
    print(pow(2, 10), pow(2, -2), pow(3, 4, 5), pow(2, -1, 7), pow(2.0, 0.5))
    try:
        print(round(float("nan")))
    except ValueError as error:
        print("ValueError:", error)
    try:
        print(round(float("inf")))
    except OverflowError as error:
        print("OverflowError:", error)
    try:
        print(chr(-1))
    except ValueError as error:
        print("ValueError:", error)
    try:
        print(ord("ab"))
    except TypeError as error:
        print("TypeError:", error)
    try:
        print(divmod(1, 0))
    except ZeroDivisionError as error:
        print("ZeroDivisionError:", error)
    xs = [1, 2, 3, 4]
    print(xs.pop(0), xs.pop(-1), xs, xs.pop(1), xs)
    try:
        xs.pop(10)
    except IndexError as error:
        print("IndexError:", error)
    empty: list[int] = []
    try:
        empty.pop(0)
    except IndexError as error:
        print("IndexError:", error)


main()
