class Circle {
  label: string;
  radius: number;
  constructor(radius: number) {
    this.label = "circle";
    this.radius = radius;
  }
  area(): number {
    if (this.radius < 0) {
      throw new Error("negative radius");
    }
    return Math.round(3.14159 * this.radius * this.radius);
  }
}
class Rect {
  label: string;
  side: number;
  constructor(side: number) {
    this.label = "rect";
    this.side = side;
  }
  area(): number {
    if (this.side < 0) {
      throw new Error("negative side");
    }
    return this.side * this.side;
  }
}
function make(round: boolean, size: number): Circle | Rect {
  return round ? new Circle(size) : new Rect(size);
}
function measure(shape: Circle | Rect): string {
  try {
    return shape.label + " " + shape.area();
  } catch (error) {
    return "failed: " + (error as Error).message;
  }
}
const shape = make(true, 2);
shape.label = shape.label + "!";
console.log(measure(shape), measure(make(false, 3)));
console.log(measure(make(true, -1)), measure(make(false, -1)));
