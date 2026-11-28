# `str.partition`/`rpartition`/`splitlines`/`expandtabs`, `zip` and
# `enumerate` over a `str` (code points), and f-string format specs built
# from nested replacement fields.

s = "key=value=x"
print(s.partition("="), s.rpartition("="), s.partition("#"), s.rpartition("#"))
head, sep, tail = "a.b.c".rpartition(".")
print(head, sep, tail, "é→ü".partition("→"))
try:
    s.partition("")
except ValueError as e:
    print("partition:", e)

t = "a\nb\r\nc\rd\x0be f\n"
# `repr` of U+2028 is CPython's `'\u2028'` escape, which the runtime does not
# write; the kept boundary is shown by name instead.
kept = [line.replace("\u2028", "<LS>") for line in t.splitlines(True)]
print(t.splitlines(), kept, "".splitlines(), "x".splitlines(), "\n\n".splitlines())
print("one\ntwo".splitlines(keepends=True))
print(repr("a\tb\tc".expandtabs()), repr("a\tbc\td".expandtabs(4)), repr("x\n\ty".expandtabs(3)))
print(repr("\t".expandtabs(2)), repr("ab\t".expandtabs(0)), repr("é\tx".expandtabs(tabsize=4)))

for a, b in zip("abc", "xyz"):
    print(a, b)
for i, ch in enumerate("hé!"):
    print(i, ch)
print(list(zip("ab", [1, 2, 3])), list(enumerate("xy", 1)), list(zip([1.5], "q")))

w = 8
p = 2
x = 3.14159
n = 42
name = "ab"
print(f"[{x:{w}.{p}f}]", f"[{name:>{w}}]", f"[{n:0{w}d}]", f"[{x:.{p}}]")
print(f"[{n:{'<' if n > 40 else '>'}{w}}]", f"[{name!r:^{w}}]", f"[{True:{w}}]")
for width in [3, 6]:
    print(f"|{'x':*^{width}}|{width:{width}}|")
