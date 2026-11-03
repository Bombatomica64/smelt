# `break` and `continue` leave a `try` through its `finally`: the clause runs
# before control reaches the loop exit or the next iteration.
def break_in_try(limit: int) -> int:
    count: int = 0
    i: int = 0
    while i < limit:
        i += 1
        try:
            if i == 2:
                break
            print("body", i)
        finally:
            count += 1
            print("finally", i)
    return count


def continue_in_try(limit: int) -> int:
    count: int = 0
    i: int = 0
    while i < limit:
        i += 1
        try:
            if i == 2:
                continue
            print("body", i)
        finally:
            count += 1
            print("finally", i)
    return count


def nested(limit: int) -> None:
    i: int = 0
    while i < limit:
        i += 1
        try:
            try:
                if i == 1:
                    continue
                break
            finally:
                print("inner", i)
        finally:
            print("outer", i)


print(break_in_try(3))
print(continue_in_try(3))
nested(3)
