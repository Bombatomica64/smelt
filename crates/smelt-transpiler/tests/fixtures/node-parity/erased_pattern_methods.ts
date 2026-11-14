// `.match`/`.search` on an erased receiver resolve by the run-time value:
// a string answers String.prototype, an object its own member, and anything
// else throws a catchable TypeError. Match arrays print with their named props.
const s: any = "a1b22";
console.log(s.match(/\d+/g), s.search(/b/), s.search("2"));
const o: any = { match: (p: string) => "custom " + p, search: (p: unknown) => 7 };
console.log(o.match("x"), o.search(/q/));
const list: any[] = ["q9", { match: (p: unknown) => typeof p }];
for (const item of list) console.log(item.match(/\d/));
const named: any = "2024-05";
const m = named.match(/(?<year>\d{4})-(?<month>\d{2})/);
console.log(m.groups.year, m.index, m[2]);
console.log(s.match("z"), s.match(/(\d)(\d)/)?.[2]);
try { const n: any = 5; n.match(/x/); } catch (e) { console.log((e as Error).constructor.name, (e as Error).message); }
try { const u: any = undefined; u.search("x"); } catch (e) { console.log((e as Error).message); }
const typed: string = "abc1";
console.log(typed.match(/\d/)?.[0]);
console.log(typed.match(/\w/), "q9".match(/\d/));
const g2: string[] = ["x"];
(g2 as any).extra = 1;
console.log(g2);
