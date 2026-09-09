// Top-level `await`. ES modules have had it since ES2022 and Node runs it;
// Smelt rejected it outright ("await expressions are only lowered inside async
// functions"), because the module body was never treated as async even though
// it IS the program's entry point.
async function greet(name: string): Promise<string> {
  return `hello ${name}`;
}

async function sum(values: number[]): Promise<number> {
  let total = 0;
  for (const value of values) {
    total = total + value;
  }
  return total;
}

const first = await greet("ada");
console.log(first);

const total = await sum([1, 2, 3]);
console.log(total);

// An await inside an expression, not just a binding.
console.log((await greet("grace")).length);
