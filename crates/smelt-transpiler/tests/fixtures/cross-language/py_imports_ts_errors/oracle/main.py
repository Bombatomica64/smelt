from guard import check

try:
    print(check(1.5))
    print(check(-2.5))
except Exception as e:
    print("caught", str(e))
