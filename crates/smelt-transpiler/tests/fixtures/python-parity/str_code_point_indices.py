def show(s: str) -> None:
    print(len(s), s.find("!"), s.rfind("b"), s[1], s[1:3], s[-2:])


show("café!")
show("a😀b!")
show("abc!")
