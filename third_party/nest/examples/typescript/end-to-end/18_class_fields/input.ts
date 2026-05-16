class Point {
  public x: number;
  private y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}
const p = new Point(2, 3);
console.log(p.x);
