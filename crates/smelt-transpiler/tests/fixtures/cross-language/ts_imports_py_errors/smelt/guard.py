def check(x: int) -> int:
    if x < 0:
        raise ValueError("negative: " + str(x))
    return x
