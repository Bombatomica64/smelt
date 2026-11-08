# Negative indexes count from the end, slices clamp and step, and an index
# past either end raises `IndexError`.
xs: list[int] = [10, 20, 30, 40, 50]
print(xs[-1], xs[-5], xs[0])
print(xs[1:3], xs[-2:], xs[:-3], xs[::2], xs[::-1], xs[-100:100], xs[4:1])
word = "héllo"
print(word[-1], word[1:3], word[::-1], word[-3:])
t = (1, 2, 3)
print(t[-1], t[0])
for bad in [5, -6]:
    try:
        print(xs[bad])
    except IndexError as e:
        print("IndexError", e)
try:
    print(word[int("9")])
except IndexError as e:
    print("IndexError", e)
print(xs[1::2], xs[-1:-4:-1], xs[3:0:-2], xs[::-2], word[::2], word[-1::-2])
step = int("0")
try:
    print(xs[::step])
except ValueError as e:
    print("ValueError", e)
