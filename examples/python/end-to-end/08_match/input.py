status: int = 2

match status:
    case 1:
        print(100)
    case 2:
        print(200)
    case _:
        print(0)
