import asyncio

from work import double


async def main() -> None:
    print(await double(21.5))


asyncio.run(main())
