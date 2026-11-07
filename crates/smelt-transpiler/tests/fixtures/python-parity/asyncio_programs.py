# A program that only uses `async def`, `await`, `asyncio.run`,
# `asyncio.gather` and `asyncio.sleep` builds without importing the program,
# and its tasks interleave at each `await` the way CPython's event loop
# schedules them.
import asyncio


async def double(n: int) -> int:
    await asyncio.sleep(0)
    return n * 2


async def chatty(name: str, steps: int) -> int:
    total = 0
    for step in range(steps):
        print(name, "step", step)
        total += step
        await asyncio.sleep(0)
    return total


async def fails() -> int:
    await asyncio.sleep(0)
    raise ValueError("async failure")


async def main() -> None:
    first = await double(1)
    doubled = await asyncio.gather(double(2), double(3), double(4))
    print(first, doubled)
    totals = await asyncio.gather(chatty("a", 3), chatty("b", 2))
    print("totals", totals)
    try:
        await fails()
    except ValueError as e:
        print("caught", e)
    print("done")


asyncio.run(main())
