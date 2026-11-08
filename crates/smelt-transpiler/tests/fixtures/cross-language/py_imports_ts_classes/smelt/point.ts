export class Point {
  x: number;
  y: number;
  labelText: string;
  constructor(x: number, y: number) { this.x = x; this.y = y; this.labelText = "p"; }
  norm2(): number { return this.x * this.x + this.y * this.y; }
  scaledBy(k: number): number { return this.x * k; }
}
