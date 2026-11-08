export function add(a: number, b: number): number { return a + b; }
export function greet(name: string): string { return "hello " + name; }
export function scale(xs: number[], k: number): number[] { return xs.map((x) => x * k); }
export function pad(s: string, width: number = 5): string { return s + ".".repeat(width - s.length); }
