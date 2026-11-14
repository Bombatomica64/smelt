# The entry module runs as `__main__`, and output written while a decorator
# runs at definition time is printed once, in source order.
def register(f):
    print("register", f.__name__)
    return f
print("start")
@register
def g() -> int:
    return 1
def main() -> None:
    print("g", g())
if __name__ == "__main__":
    main()
main()
