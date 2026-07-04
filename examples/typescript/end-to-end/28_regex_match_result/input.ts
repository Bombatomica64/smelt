// Exercises the concrete SmeltMatch type through consumer-side reads.
//
// `String.prototype.matchAll` yields typed match values whose reads are typed
// against SmeltMatch (no erased SmeltUnknown): the whole match ([0]), a named
// capture group (.groups.letter), an optional numbered group that is present in
// some matches and missing in others ([2]), the match offset (.index), and the
// searched input (.input). Array destructuring of a match value binds the
// numbered groups, and `RegExp.prototype.exec` reads a single optional match.
const pattern = /(?<letter>[a-z])(\d)?/g;

const matches = "a1 b c3".matchAll(pattern);
for (const found of matches) {
  const whole = found[0];
  const letter = found.groups.letter;
  const digit = found[2];
  console.log(whole);
  console.log(letter);
  console.log(digit);
  console.log(found.index);
  console.log(found.input);
  const [full, first, second] = found;
  console.log(full);
  console.log(first);
  console.log(second);
}

const single = /(?<year>\d{4})-(\d{2})/;
const found = single.exec("2024-07");
if (found !== null) {
  console.log(found[0]);
  console.log(found.groups.year);
  console.log(found[1]);
  console.log(found[2]);
  console.log(found.index);
}
