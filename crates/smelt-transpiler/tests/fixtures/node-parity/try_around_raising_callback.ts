function boom(value: number): number {
  if (value === 2) {
    throw new Error("bad " + value);
  }
  return value * 2;
}
function viaMap(values: number[]): string {
  try {
    return values.map(boom).join(",");
  } catch (error) {
    return "caught " + (error as Error).message;
  }
}
function viaSort(values: number[]): string {
  try {
    const sorted = [...values].sort((a, b) => {
      if (a === 3 || b === 3) {
        throw new Error("no order");
      }
      return a - b;
    });
    return sorted.join(",");
  } catch (error) {
    return "caught " + (error as Error).message;
  }
}
class Box {
  items = [3, 1];
  order(): string {
    try {
      this.items.sort((a, b) => {
        if (a + b === 4) {
          throw new Error("field");
        }
        return a - b;
      });
      return "sorted";
    } catch (error) {
      return "caught " + (error as Error).message;
    }
  }
}
function inPlace(values: number[]): string {
  const local = values;
  try {
    local.sort((a, b) => {
      if (a === 5 || b === 5) {
        throw new Error("five");
      }
      return a - b;
    });
    return local.join(",");
  } catch (error) {
    return "caught " + (error as Error).message;
  }
}
console.log(new Box().order(), inPlace([2, 1]), inPlace([5, 1]));
console.log(viaMap([1, 3]), viaMap([1, 2, 3]));
console.log(viaSort([2, 1]), viaSort([1, 3, 2]));
