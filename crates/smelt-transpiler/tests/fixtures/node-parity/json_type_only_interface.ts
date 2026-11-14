// `JSON.stringify` of interfaces declared in a module imported only for its
// types (date-fns' `Duration`, an interface of optional numbers). An absent
// optional property is omitted, as in Node.
import type { Duration, Span } from "./types.ts";

function build(years: number, days: number): Duration {
  const duration: Duration = {};
  if (years) duration.years = years;
  if (days) duration.days = days;
  return duration;
}

function main(): void {
  console.log(JSON.stringify(build(1, 3)));
  console.log(JSON.stringify(build(0, 0)));
  console.log(JSON.stringify([build(2, 0), { months: 2.5, label: "x" }]));
  const span: Span = { from: build(0, 4), parts: [build(1, 0)] };
  console.log(JSON.stringify(span));
  console.log(JSON.stringify(span, null, 2));
}

main();
