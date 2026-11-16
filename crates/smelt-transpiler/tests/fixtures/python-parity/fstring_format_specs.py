# f-string format specs follow CPython's mini-language for int, float, str and
# bool (fill, alignment, sign, z, #, zero padding, width, grouping, precision,
# type), `{x=}` writes its own source, and a spec the type does not take
# raises ValueError.
def main() -> None:
    x: float = 1234.5678
    tiny: float = -0.0001
    n: int = -1234567
    s: str = "hi"
    print(f"{x:.2f}|{x:10.3f}|{x:<10.1f}|{x:^12,.2f}|{x:+.3e}|{x:.3g}|{x:g}|{x:.10g}|{x:e}|{x:%}|{x:.1%}")
    print(f"{x:010.2f}|{tiny:z.1f}|{x:#.0f}|{tiny:#g}|{x:.3}|{x:,}|{x:_}|{x:>12}|{x:=+12,.1f}|{tiny:.2f}")
    print(f"{n:d}|{n:5d}|{n:,}|{n:_}|{n:x}|{n:#x}|{n:X}|{n:#o}|{n:b}|{n:#010b}|{n:_b}|{n:08,d}|{65:c}|{n:e}|{n:.2f}")
    print(f"{s:5}|{s:>5}|{s:^6}|{s:.1}|{s:*^7.1}|{True:>5}|{2 ** 70:,}|{255:08b}|{7:+}|{7: }")
    print(f"{x=}|{n = }|{s=!s}|{x=:.1f}|{1 + 2=}")
    try:
        print(f"{x:d}")
    except ValueError as e:
        print("ValueError:", e)
    try:
        print(f"{s:,}")
    except ValueError as e:
        print("ValueError:", e)


main()
