// An erased object recovered at a record type whose field is `T | null`: a
// present `null` (or `undefined`) is the field's absent arm, not a value to
// convert. Reading `score` off `{ score: null }` at `number | null` must be
// `null`, not `NaN` (the number conversion of `null`). The value is erased
// because it comes from `JSON.parse`, a genuine dynamic boundary.
interface Entry {
  name: string;
  score: number | null;
  note: string | null;
}

const entries = JSON.parse('[{"name":"a","score":3,"note":"x"},{"name":"b","score":null,"note":null}]') as Entry[];
for (const entry of entries) {
  console.log(entry.name, entry.score === null ? 'null' : entry.score, entry.note === null ? 'null' : entry.note);
}
const one = JSON.parse('{"name":"c","score":null,"note":"y"}') as Entry;
console.log(one.score === null, one.score ?? -1, one.note);
