export function check(x: number): number {
  if (x < 0) { throw new Error("negative: " + String(x)); }
  return x;
}
