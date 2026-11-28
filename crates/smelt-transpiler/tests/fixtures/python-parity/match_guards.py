# `match` with guards, captures, `|` alternatives and `as` bindings: cases
# are tried in order, a guard runs only when its pattern matched, and a
# capture is bound before its guard runs (it stays bound when the guard
# fails).


def classify(n: int) -> str:
    match n:
        case 0:
            return "zero"
        case 1 | 2 | 3 as small if small % 2 == 1:
            return f"small odd {small}"
        case 1 | 2 | 3:
            return "small even"
        case x if x < 0:
            return f"negative {-x}"
        case x if x % 2 == 0:
            return "even"
        case _:
            return "odd"


print([classify(v) for v in [0, 1, 2, 3, -4, 10, 11]])


def command(text: str) -> str:
    match text:
        case "quit" | "exit":
            return "bye"
        case "help":
            return "usage"
        case other if other.startswith("go "):
            return "moving " + other[3:]
        case other:
            return "unknown " + other


print(command("exit"), command("help"), command("go north"), command("dance"))

calls: list[str] = []


def check(label: str, result: bool) -> bool:
    calls.append(label)
    return result


value = 5
match value:
    case 4 if check("first", True):
        print("four")
    case seen if check("second", False):
        print("unreachable")
    case 5 if check("third", True):
        print("five, after", seen)
print(calls)

flag = True
match flag:
    case True:
        print("true")
    case False:
        print("false")
