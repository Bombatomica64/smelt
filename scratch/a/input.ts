const fact = function fact(n: number): number {
  return n <= 1 ? 1 : n * fact(n - 1);
};
const greet = function inner(name: string): string {
  return "hi " + name;
};
console.log(fact(5));
console.log(greet("bob"));
