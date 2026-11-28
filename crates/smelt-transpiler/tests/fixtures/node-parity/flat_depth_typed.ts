console.log([[1, 2], [3, [4, 5]]].flat(2), [[1], [2, [3]]].flat(1), [1, [2, [3, [4]]]].flat(Infinity));
const words: string[][][] = [[["a", "b"], ["c"]], [["d"]]];
console.log(words.flat(2).join(","), words.flat(1).length, words.flat(0).length);
const pairs: [string, number[]][] = [["x", [1, 2]], ["y", [3]]];
console.log(pairs.flat(2));
const nested: (number | (number | number[])[])[] = [1, [2, [3]]];
function flatTo(depth: number) { return nested.flat(depth); }
console.log(flatTo(0).length, flatTo(1).length, flatTo(5).length);
