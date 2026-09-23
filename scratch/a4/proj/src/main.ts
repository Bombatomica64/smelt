const plain = (n: number): number => n + 1;
const named = function tick(n: number): number {
  return n <= 0 ? 0 : 1 + tick(n - 1);
};
function run(): void {
  console.log(plain(3));
  console.log(named(3));
}
console.log(named(4));
run();
