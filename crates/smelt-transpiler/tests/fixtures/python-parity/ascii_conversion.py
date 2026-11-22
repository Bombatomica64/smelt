print(ascii("plain"), ascii("café ☕ 😀"), ascii(["é", "1"]))
print(ascii(1.5), ascii(None), ascii("tab\there"), ascii("ÿĀ"))
s = "naïve"
print(f"{s!a} {s!r} {s!s}")
print(f"[{s!a:>12}]")
print("%a|%5a|%-9a|" % (s, "x", "é"))
