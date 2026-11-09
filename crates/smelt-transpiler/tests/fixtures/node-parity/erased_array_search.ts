// Array searches on a receiver only known at run time.

function search(list: any, needle: any): unknown[] {
  return [list.indexOf(needle), list.lastIndexOf(needle), list.includes(needle)];
}

console.log(search([7, 8, 7], 7).join(","));
console.log(search([7, 8, 7], 8).join(","));
console.log(search([7, 8, 7], 9).join(","));
console.log(search(["a", "b"], "a").join(","));

function from(list: any, needle: any, start: number): number {
  return list.indexOf(needle, start);
}

console.log(from([1, 2, 1], 1, 0), from([1, 2, 1], 1, 1), from([1, 2, 1], 1, -1), from([1, 2, 1], 1, 9));
