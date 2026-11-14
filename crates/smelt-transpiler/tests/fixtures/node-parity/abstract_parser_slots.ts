// An abstract generic parser hierarchy in the shape of date-fns' `parse`:
// subclasses fill the base's abstract and overridable members, the base's
// `run` hands `this.set` / `this.validate` to a generic `ValueSetter<Value>`,
// a setter's callback mutates its `flags` record and may return the state as
// a tuple, and parsers of different value types share one `Parser<any>[]`.
interface Flags {
  stamped?: boolean;
  era?: number;
}
type ParseResult<V> = { value: V; rest: string } | null;

class ValueSetter<Value> {
  value: Value;
  validateValue: (value: Value) => boolean;
  setValue: <D extends Date>(date: D, flags: Flags, value: Value) => D | [D, Flags];
  priority: number;
  constructor(
    value: Value,
    validateValue: (value: Value) => boolean,
    setValue: <D extends Date>(date: D, flags: Flags, value: Value) => D | [D, Flags],
    priority: number,
  ) {
    this.value = value;
    this.validateValue = validateValue;
    this.setValue = setValue;
    this.priority = priority;
  }
  validate(): boolean {
    return this.validateValue(this.value);
  }
  set<D extends Date>(date: D, flags: Flags): D | [D, Flags] {
    return this.setValue(date, flags, this.value);
  }
}

abstract class Parser<Value> {
  abstract priority: number;
  run(s: string): { setter: ValueSetter<Value>; rest: string } | null {
    const result = this.parse(s);
    if (!result) return null;
    return {
      setter: new ValueSetter<Value>(result.value, this.validate, this.set, this.priority),
      rest: result.rest,
    };
  }
  protected abstract parse(s: string): ParseResult<Value>;
  protected validate(_value: Value): boolean {
    return true;
  }
  protected abstract set<D extends Date>(date: D, flags: Flags, value: Value): D | [D, Flags];
}

class YearParser extends Parser<number> {
  priority = 130;
  parse(s: string): ParseResult<number> {
    const m = /^\d{4}/.exec(s);
    return m ? { value: Number(m[0]), rest: s.slice(4) } : null;
  }
  override validate(value: number): boolean {
    return value > 0;
  }
  set<D extends Date>(date: D, flags: Flags, value: number): D | [D, Flags] {
    flags.era = value < 1000 ? 0 : 1;
    if (flags.era === 0) {
      date.setUTCFullYear(value, 0, 1);
      return [date, { stamped: true, era: 0 }];
    }
    date.setUTCFullYear(value, 0, 1);
    return date;
  }
}

class MonthParser extends Parser<number> {
  priority = 110;
  parse(s: string): ParseResult<number> {
    const m = /^\d\d/.exec(s);
    return m ? { value: Number(m[0]) - 1, rest: s.slice(2) } : null;
  }
  override validate(value: number): boolean {
    return value >= 0 && value < 12;
  }
  set<D extends Date>(date: D, _flags: Flags, value: number): D {
    date.setUTCMonth(value, 1);
    return date;
  }
}

class WordParser extends Parser<string> {
  priority = 10;
  parse(s: string): ParseResult<string> {
    const m = /^[a-z]+/.exec(s);
    return m ? { value: m[0], rest: s.slice(m[0].length) } : null;
  }
  set<D extends Date>(date: D, flags: Flags, value: string): D {
    flags.stamped = value === "now";
    return date;
  }
}

function apply(parsers: Parser<number>[], input: string): string {
  let rest = input;
  let date = new Date(Date.UTC(2000, 0, 1));
  let flags: Flags = {};
  for (const parser of parsers) {
    const parsed = parser.run(rest);
    if (!parsed) return "invalid at " + rest;
    if (!parsed.setter.validate()) return "rejected " + parsed.setter.value;
    rest = parsed.rest;
    const out = parsed.setter.set(date, flags);
    if (Array.isArray(out)) {
      date = out[0];
      flags = out[1];
    } else {
      date = out;
    }
  }
  return `${date.getUTCFullYear()}-${date.getUTCMonth() + 1} era=${flags.era} stamped=${flags.stamped === true} rest=[${rest}]`;
}

function main(): void {
  console.log(apply([new YearParser(), new MonthParser()], "202403x"));
  console.log(apply([new YearParser()], "abc"));
  console.log(apply([new MonthParser(), new YearParser()], "121999"));
  console.log(apply([new YearParser(), new MonthParser()], "202413"));
  console.log(apply([new YearParser()], "0999"));
  // date-fns keeps its parsers as `Parser<any>`: `unknown` would break strict
  // function-type variance on `validateValue`.
  const mixed: Parser<any>[] = [new WordParser(), new YearParser()];
  for (const parser of mixed) {
    const parsed = parser.run("now2024");
    console.log(parsed ? `${parser.priority} ${parsed.setter.validate()} [${parsed.rest}]` : `${parser.priority} none`);
  }
  const flags: Flags = {};
  const word = new WordParser().run("now!");
  if (word) word.setter.set(new Date(0), flags);
  console.log(flags.stamped === true);
}

main();
