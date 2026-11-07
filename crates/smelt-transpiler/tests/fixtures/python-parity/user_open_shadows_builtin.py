# A user `open` is the user's function, not the builtin.
def open(path: str) -> str:
    return "opened " + path


print(open("notes.txt"), len(open("x")))
