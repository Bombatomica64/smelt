function delay<T>(ms: number, value: T): Promise<T> {
  return new Promise((resolve) => setTimeout(() => resolve(value), ms));
}
function fail(ms: number, message: string): Promise<number> {
  return new Promise((_resolve, reject) => setTimeout(() => reject(new Error(message)), ms));
}
async function main(): Promise<void> {
  const first = await Promise.any([fail(5, "a"), delay(20, 2), delay(10, 3)]);
  console.log("first", first);
  try {
    await Promise.any([fail(5, "x"), fail(1, "y")]);
    console.log("unreachable");
  } catch (e) {
    const error = e as AggregateError;
    console.log(error.name, error.message, error.errors.length, (error.errors[0] as Error).message, (error.errors[1] as Error).message);
  }
}
main();
