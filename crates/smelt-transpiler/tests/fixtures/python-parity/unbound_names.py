# A Python name has no value until an assignment binds it. Reading it before
# then raises: `UnboundLocalError` for a function local, `NameError` for a
# free variable of an enclosing function or a module-level name. All three are
# ordinary exceptions an `except` clause catches; a read on a path that did
# assign the name sees its value.


def branch_assigned(flag: bool) -> str:
    if flag:
        x = 1
    try:
        return "x=" + str(x)
    except UnboundLocalError as error:
        return "caught UnboundLocalError: " + str(error)


def caught_as_name_error(flag: bool) -> str:
    if flag:
        y = "set"
    try:
        return y
    except NameError as error:
        return "caught NameError: " + str(error)


def free_variable(flag: bool) -> str:
    if flag:
        captured = 41

    def read() -> int:
        return captured + 1

    try:
        return "read " + str(read())
    except NameError as error:
        return "caught NameError: " + str(error)


def uncaught(flag: bool) -> int:
    if flag:
        z = 5
    return z


print(branch_assigned(True))
print(branch_assigned(False))
print(caught_as_name_error(True))
print(caught_as_name_error(False))
print(free_variable(True))
print(free_variable(False))
try:
    print(uncaught(False))
except UnboundLocalError as error:
    print("propagated UnboundLocalError: " + str(error))


def pick() -> bool:
    return False


if pick():
    module_name = "bound"
try:
    print(module_name)
except NameError as error:
    print("module NameError: " + str(error))
