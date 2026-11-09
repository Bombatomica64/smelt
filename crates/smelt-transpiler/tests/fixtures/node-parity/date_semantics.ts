// Dates and URLs observed through UTC-stable values.

const iso = new Date(0).toISOString();
console.log(iso);
const url = new URL("https://example.com:8443/a?q=1");
console.log(new URL("https://example.com/a?q=1").hostname, url.host, new URL("https://example.com:443/").host);
console.log("a  b".replace(new RegExp("\\s+"), "-"), "a  b".replace(RegExp("\\s+"), "-"), "a  b  c".replace(/\s+/, "-"));

function isoFrom(value: unknown): string {
  return new Date(value as any).toISOString();
}
console.log(isoFrom(1000), isoFrom("2020-01-02T03:04:05.000Z"));

function bump<D extends Date>(date: D): string {
  date.setUTCFullYear(2024);
  return date.toISOString();
}
console.log(bump(new Date(0)));

function isoAll(values: any[]): string[] {
  return values.map(value => value.toISOString());
}
console.log(isoAll([new Date(5)]).join("|"));

console.log(new Date("Wed Jul 02 2014 05:30:15 GMT+0600").toISOString());

function yearRoundTrip(value: unknown): number {
  const date = new Date(0);
  date.setUTCFullYear(value as number);
  return date.getUTCFullYear();
}
console.log(yearRoundTrip(1999));

function apply(isTwoDigitYear: boolean, year: number, date: Date): Date {
  if (isTwoDigitYear) {
    const normalizedTwoDigitYear = year + 2000;
    date.setUTCFullYear(normalizedTwoDigitYear, 0, 1);
    return date;
  }
  return date;
}
console.log(apply(true, 24, new Date(0)).toISOString(), apply(false, 24, new Date(0)).toISOString());

interface Localize {
  month: (value: number) => string;
  preprocessor?: (date: Date, parts: string[]) => string[];
}
interface Locale {
  localize: Localize;
}
function format(options?: { locale?: Locale }): string {
  const parts = ["a", "b"];
  const pre = options?.locale?.localize.preprocessor;
  return pre ? pre(new Date(Date.UTC(2020, 0, 1)), parts).join("|") : "none";
}
const customLocale = {
  localize: {
    month: (value: number) => String(value),
    preprocessor: (date: Date, parts: string[]) => (date.getUTCDate() === 1 ? parts : []),
  },
};
console.log(format({ locale: customLocale }), format());
