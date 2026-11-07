// `String` of an erased array joins its elements like `Array.prototype.join`.
function show(value: unknown): string { return String(value); }
console.log(show([1, 2, 3]), show(["a", ["b", "c"]]), show([null, undefined, 4]), show([]), show({}), show(null), show(undefined), show(4));
