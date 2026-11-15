// A fact a branch proves inside a loop or `forEach` body holds for that
// iteration's branch only (date-fns `closestIndexTo`). Each loop sits in a
// branch of its own, whose narrowing scope a leaked fact would reach.
function closestIndex(target: number, values: number[]): number | undefined {
  let result: number | undefined;
  let minDistance: number;
  if (values.length >= 0) {
    values.forEach((value, index) => {
      if (isNaN(value)) {
        result = NaN;
        minDistance = NaN;
        return;
      }
      const distance = Math.abs(target - value);
      if (result == null || distance < minDistance) {
        result = index;
        minDistance = distance;
      }
    });
    return result;
  }
  return undefined;
}

function largest(values: number[]): number | undefined {
  let best: number | undefined;
  if (values.length >= 0) {
    values.forEach((value) => {
      if (best == null) best = value;
      if (value > best) best = value;
    });
    return best;
  }
  return undefined;
}

function firstLabel(items: Array<{ label?: string }>): string {
  let found: string | undefined;
  if (items.length >= 0) {
    for (const item of items) {
      if (found == null) found = "";
      if (item.label === undefined) continue;
      found = found + item.label;
    }
    return found ?? "none";
  }
  return "none";
}

function* labels(): Generator<string> {
  yield "x";
  yield "y";
}

function joinLabels(skip: boolean): string {
  let joined: string | undefined;
  if (!skip) {
    for (const label of labels()) {
      if (joined == null) joined = "";
      joined = joined + label;
    }
    return joined ?? "none";
  }
  const empty = new Set<string>();
  if (skip) {
    for (const label of empty) {
      if (joined == null) joined = "";
      joined = joined + label;
    }
    return joined ?? "none";
  }
  return "none";
}

function* nothing(): Generator<string> {}

function emptyGenerator(): string {
  let joined: string | undefined;
  if (joined === undefined) {
    for (const label of nothing()) {
      if (joined == null) joined = "";
      joined = joined + label;
    }
    return joined ?? "none";
  }
  return "none";
}

function countKeys(record: Record<string, number | undefined>): number | undefined {
  let last: number | undefined;
  if (Object.keys(record).length >= 0) {
    for (const key in record) {
      if (last == null) last = 0;
      last = last + (record[key] ?? 0);
    }
    return last;
  }
  return undefined;
}

function sumUntil(values: Array<number | null>): number | undefined {
  let sum: number | undefined;
  if (values.length >= 0) {
    for (let i = 0; i < values.length; i++) {
      if (sum == null) sum = 0;
      const value = values[i];
      if (value === null) break;
      sum += value;
    }
    return sum;
  }
  return undefined;
}

console.log(largest([3, 9, 4]), largest([]));
console.log(closestIndex(10, [1, 12, 9, 30]), closestIndex(10, [1, NaN, 9]), closestIndex(5, []));
console.log(firstLabel([{}, { label: "a" }, {}, { label: "b" }]), firstLabel([]));
console.log(joinLabels(false), joinLabels(true), emptyGenerator());
console.log(countKeys({ a: 1, b: undefined, c: 2 }), countKeys({}));
console.log(sumUntil([1, 2, null, 4]), sumUntil([]));
