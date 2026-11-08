export class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  norm2(): number { return this.x * this.x + this.y * this.y; }
}
