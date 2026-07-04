// Exercises the concrete SmeltMatch runtime type through String.prototype.matchAll:
// the whole match ([0]), a named capture group (.groups.letter), an optional
// numbered group that is present in some matches and missing in others ([2]),
// and the match offset (.index).
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
}
