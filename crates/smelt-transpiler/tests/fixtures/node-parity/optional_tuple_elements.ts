type Rgb = [r: number, g: number, b: number, a?: number];
const c1: Rgb = [1, 2, 3];
const c2: Rgb = [1, 2, 3, 0.5];
console.log(c1.length, c2.length, c2[3] ?? 1, c1[3] ?? 1);
