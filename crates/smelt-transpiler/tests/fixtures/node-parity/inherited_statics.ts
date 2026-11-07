class Shape {
  static sides = 0;
  static label = "shape";
  static describe(): string {
    return "a " + Shape.label;
  }
}
class Square extends Shape {
  static sides = 4;
}
class Tile extends Square {
  static glaze = "blue";
}
console.log(Square.sides, Square.label, Square.describe());
console.log(Tile.sides, Tile.label, Tile.glaze, Tile.describe());
console.log(Shape.sides);
