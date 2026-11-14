// A reducer that can throw: its fold must still thread plain accumulator
// values, and a throw from inside it must reach the enclosing `catch`.

function step(acc: number, value: number): number {
  if (value < 0) {
    throw new Error(`negative ${value}`);
  }
  return acc + value;
}

function total(values: number[]): string {
  try {
    const sum = values.reduce((acc, value) => step(acc, value), 0);
    return `sum ${sum}`;
  } catch (error) {
    return `caught ${(error as Error).message}`;
  }
}

function joined(words: string[]): string {
  try {
    return words.reduce((acc, word) => {
      if (word.length === 0) {
        throw new Error("empty word");
      }
      return `${acc}-${word}`;
    });
  } catch (error) {
    return `caught ${(error as Error).message}`;
  }
}

console.log(total([1, 2, 3]));
console.log(total([1, -2, 3]));
console.log(total([]));
console.log(joined(["a", "b", "c"]));
console.log(joined(["a", "", "c"]));
console.log([4, 5].reduce(step, 10));
