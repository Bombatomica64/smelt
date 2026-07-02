export function celsius(fahrenheit: number): number {
  return ((fahrenheit - 32) * 5) / 9;
}

export function fibonacci(n: number): number {
  let a = 0;
  let b = 1;
  for (let i = 0; i < n; i++) {
    const next = a + b;
    a = b;
    b = next;
  }
  return a;
}
