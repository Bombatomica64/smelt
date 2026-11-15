// An exported object literal of methods: importers index it with a run-time
// key, test the looked-up member for presence, and pass the whole object on.
export const formatters = {
  y(value: number, width: number): string {
    return String(value % 100).padStart(width, "0");
  },
  M(value: number, width: number): string {
    return String(value + 1).padStart(width, "0");
  },
  d(value: number, width: number): string {
    return String(value).padStart(width, "0");
  },
};

// Members of different shapes: the value is still the object, not a placeholder.
export const helpers = {
  label(value: number): string {
    return `#${value}`;
  },
  measure(text: string, extra: number): number {
    return text.length + extra;
  },
};

// A data object and a locale-like object whose members are run-time values:
// each is ONE object, whoever reads it.
export const defaults = { weekStartsOn: 0, firstWeekContainsDate: 1 };

export interface Locale {
  code: string;
  ordinal: (value: number) => string;
  options: { weekStartsOn: number };
}

export const en: Locale = {
  code: "en",
  ordinal: (value) => `${value}th`,
  options: { weekStartsOn: 0 },
};
export const eo: Locale = {
  code: "eo",
  ordinal: (value) => `${value}-a`,
  options: { weekStartsOn: 1 },
};

// A table annotated with an index signature: every key types as a function,
// so only a presence test tells a missing token apart (date-fns `format`).
export const tokens: { [token: string]: (value: number) => string } = {
  a: function (value) {
    return `a${value}`;
  },
  b: function (value) {
    return `b${value}`;
  },
};

// A locale whose members are computed when the module runs: an object-valued
// const item, still one object.
function germanOrdinal(value: number): string {
  return `${value}.`;
}
function makeOptions(weekStartsOn: number): { weekStartsOn: number } {
  return { weekStartsOn };
}
export const de: Locale = {
  code: "de",
  ordinal: germanOrdinal,
  options: makeOptions(1),
};
