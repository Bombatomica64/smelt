export interface Shape {
  side: number;
}
export const unit = 2;
export const label = "sq";
export function area(side: number): number {
  return side * side * unit;
}
