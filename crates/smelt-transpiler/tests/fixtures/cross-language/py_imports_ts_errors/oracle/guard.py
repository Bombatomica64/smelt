def check(x: float) -> float:
    if x < 0:
        raise Exception("negative: " + str(x))
    return x
