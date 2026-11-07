# `x /= n` on an `int` rebinds the name to a `float`: earlier reads saw the
# `int`, later ones see the `float`. `//=` keeps an `int`.

def halve(n: int) -> float:
    total = n
    print(total)
    total /= 2
    total += 1
    return total


z = 9
print(z)
z /= 3
print(z, z * 2)
print(halve(5))
count = 7
count //= 2
print(count)
