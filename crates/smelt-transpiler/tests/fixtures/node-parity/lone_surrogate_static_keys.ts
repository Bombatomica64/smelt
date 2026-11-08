// A static string key holding a lone surrogate stays exact wherever it is a
// runtime key: an inferred object literal, a destructuring pattern, a constant
// list, and Object.assign. Two different halves are two different keys.
const show = (key: string): string => key.length + ":" + key.charCodeAt(0).toString(16);
const inferred = { "\ud83d": 4, "\ude00": 5, plain: 6 };
console.log(Object.keys(inferred).map(show).join(" "), JSON.stringify(inferred));
const record: Record<string, number> = { "\ud83d": 7, "\ude00": 8, rest: 9 };
const { "\ud83d": first, ...others } = record;
console.log(first, Object.keys(others).map(show).join(" "));
const LIST = ["\ud83d", "\ude00"];
console.log(LIST.map(show).join(" "));
const merged = Object.assign({} as Record<string, number>, { "\ud83d": 1 });
console.log(Object.keys(merged).map(show).join(" "), "\ud83d" in merged);
