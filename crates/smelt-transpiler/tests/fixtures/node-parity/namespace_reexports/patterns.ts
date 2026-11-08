import { matcher } from "./symbols.ts";
export { matcher };
export function map(key: string, value: number): string { return `${key}=${value}`; }
function box(n: number): { n: number } { return { n }; }
export const boxed = box(3);
export const one = 1;
