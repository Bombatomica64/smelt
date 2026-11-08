// TypeScript enum semantics: numeric auto-increment from 0 and from the last
// explicit number, string members, the reverse mapping of numeric members, and
// an enum declared inside a function body.
enum Color { Red, Green = 5, Blue }
enum Direction { Up = "UP", Down = "DOWN" }
enum Twice { First = 1, Again = 1 }
console.log(Color.Red, Color.Green, Color.Blue, Direction.Up, Direction.Down);
console.log(Color[0], Color[Color.Blue], Twice[1]);
function nameOf(value: number): string { return Color[value]; }
console.log(nameOf(5), nameOf(6));
function label(color: Color): string { return color === Color.Green ? "green" : "not green"; }
console.log(label(Color.Green), label(Color.Blue));
function local(): string {
  enum Level { Low = 10, Mid, High }
  enum Mode { On = "on", Off = "off" }
  return `${Level.Low} ${Level.Mid} ${Level.High} ${Level[11]} ${Mode.Off}`;
}
console.log(local());
const pick = (on: boolean): string => {
  enum Switch { Off, On }
  return Switch[on ? Switch.On : Switch.Off];
};
console.log(pick(true), pick(false));
