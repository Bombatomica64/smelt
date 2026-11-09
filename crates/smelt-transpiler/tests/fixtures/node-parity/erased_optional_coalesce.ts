// `??` tests the value of an optional chain on an erased receiver, so a missing
// property or a nullish receiver takes the fallback and a `null` value stays
// distinguishable from a missing one.
function raw(value: unknown): any {
  return value;
}
const items: any[] = [{ times: 1 }, null, {}, { times: null }, { times: 0 }];
console.log(items.map((item: any) => item?.times ?? 9).join(","));
const empty: any = raw({});
console.log(empty?.x ?? "fallback", empty?.x?.y ?? "deep", empty?.x === undefined);
const holdsNull: any = raw({ x: null });
console.log(holdsNull?.x ?? "from-null", holdsNull?.x === null);
const nothing: any = raw(null);
console.log(nothing?.x ?? "from-receiver");
function typed(value: any): number {
  const read: number = value?.n ?? 4;
  return read;
}
console.log(typed({}), typed({ n: 5 }), typed(undefined));
