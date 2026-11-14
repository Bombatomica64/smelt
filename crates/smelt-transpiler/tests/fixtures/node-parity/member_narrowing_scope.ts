// A narrowing proved in one class member does not reach a sibling member's
// same-named local. `if (!m) return ...` leaves `m` non-null for the rest of
// `DigitParser.parse` only; `WordParser.parse` declares its own `m`, which may
// be null, so its `m ? .. : ..` must still test it.
class DigitParser {
  parse(s: string): number {
    const m = /^\d+/.exec(s);
    if (!m) return -1;
    return m[0].length;
  }
}

class WordParser {
  parse(s: string): number {
    const m = /^[a-z]+/.exec(s);
    return m ? m[0].length : -2;
  }
}

class Pair {
  first: number;
  second: number;
  constructor(s: string) {
    const m = /^(\d)(\d)/.exec(s);
    if (!m) {
      this.first = 0;
      this.second = 0;
      return;
    }
    this.first = Number(m[1]);
    this.second = Number(m[2]);
  }
  label(s: string): string {
    const m = /^[A-Z]/.exec(s);
    return m ? m[0] : "none";
  }
}

function main(): void {
  const digits = new DigitParser();
  const words = new WordParser();
  console.log(digits.parse("123x"), digits.parse("x"));
  console.log(words.parse("7"), words.parse("abc1"));
  const pair = new Pair("42");
  console.log(pair.first, pair.second, pair.label("x"), pair.label("Yes"));
  console.log(new Pair("x").first);
}

main();
