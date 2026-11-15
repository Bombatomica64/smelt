import { de, defaults, en, eo, formatters, helpers, tokens, type Locale } from "./formatters.ts";

// date-fns `lightFormat`: index the imported formatter table with a pattern
// character and only call what is there. The table's type says every key is
// a function, so the presence test is the only thing standing between an
// unknown character and a call of `undefined`.
function render(pattern: string, parts: { y: number; M: number; d: number }): string {
  let out = "";
  let index = 0;
  while (index < pattern.length) {
    const token = pattern[index];
    let width = 1;
    while (pattern[index + width] === token) width++;
    const formatter = formatters[token as keyof typeof formatters];
    if (formatter) {
      out += formatter(parts[token as keyof typeof parts], width);
    } else {
      out += token.repeat(width);
    }
    index += width;
  }
  return out;
}

console.log(render("yyyy-MM-dd", { y: 2024, M: 0, d: 7 }));
console.log(render("d/M/yy hh:mm", { y: 1999, M: 11, d: 31 }));

// The table as a value: enumeration, membership and identity agree with Node.
const table = formatters;
console.log(Object.keys(table).join(","), "y" in table, "x" in table, table === formatters);
console.log(typeof formatters.M, typeof (formatters as Record<string, unknown>)["q"]);

// A table whose members differ in shape is still the exported object.
const keys = Object.keys(helpers);
console.log(keys.join(","), helpers.label(3), helpers.measure("abc", 2));
const pick = (name: string): boolean => Object.prototype.hasOwnProperty.call(helpers, name);
console.log(pick("label"), pick("missing"));

// date-fns `setDefaultOptions`: a module keeps the locale it was handed and a
// reader compares it with the imported binding.
let current: { locale?: Locale } = {};
function setDefaultLocale(locale: Locale): void {
  current = { locale };
}
setDefaultLocale(eo);
console.log(current.locale === eo, current.locale === en, current.locale?.ordinal(2));
const all: Locale[] = [en, eo];
console.log(all.indexOf(eo), all.includes(en), new Set([en, en, eo]).size);
const sameDefaults = defaults;
console.log(sameDefaults === defaults, defaults.weekStartsOn, Object.keys(defaults).length);

function expand(pattern: string): string {
  let out = "";
  for (const ch of pattern) {
    if (tokens[ch]) {
      out += tokens[ch](1);
    } else {
      out += ch;
    }
  }
  return out;
}
console.log(expand("a-b c"));
setDefaultLocale(de);
console.log(current.locale === de, current.locale?.ordinal(3), all.includes(de), de.options === de.options);
