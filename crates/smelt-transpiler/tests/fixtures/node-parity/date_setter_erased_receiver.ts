// A Date setter on a value whose static type is an erased generic (`D extends
// Date` reached through a union) reads the receiver's timestamp, then writes
// the new one into that same receiver: one operand, two uses. Every alias of
// the receiver sees the write, as in JavaScript.
function shiftDays<D extends Date>(date: D | number, amount: number): D {
  const d = (typeof date === "number" ? new Date(date) : date) as D;
  const alias = d;
  d.setUTCDate(d.getUTCDate() + amount);
  d.setUTCHours(d.getUTCHours() + 1);
  console.log(alias.getTime() === d.getTime(), alias.getUTCDate(), alias.getUTCHours());
  return d;
}

function main(): void {
  console.log(shiftDays(Date.UTC(2024, 1, 27, 22, 0, 0), 2).getUTCDate());
  console.log(shiftDays(Date.UTC(2024, 0, 31, 23, 0, 0), 0).getUTCMonth());
  console.log(shiftDays(NaN, 1).getTime());
}

main();
