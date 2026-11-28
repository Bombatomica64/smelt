// Local-time `Date` methods read the process time zone (`TZ`) as Node does:
// getters, setters and constructor parts across DST changes, the string
// methods with the zone's long name, and the `en-US` locale forms. Run in
// several zones (`verify_node_parity_in_zones`); the UTC methods stay UTC.
const d = new Date(2024, 0, 15, 10, 30, 45, 123);
console.log(d.getTime(), d.getTimezoneOffset());
console.log(d.getFullYear(), d.getMonth(), d.getDate(), d.getDay(), d.getHours(), d.getMinutes());
console.log(d.toString());
console.log(d.toDateString(), "|", d.toTimeString());
console.log(d.toLocaleDateString(), "|", d.toLocaleTimeString(), "|", d.toLocaleString());
const s = new Date(Date.UTC(2024, 6, 1, 12));
console.log(s.getHours(), s.getTimezoneOffset(), s.toString());
s.setHours(23, 59);
console.log(s.toISOString(), s.getDate());
const p = new Date("2024-03-10T12:00:00");
console.log(p.toISOString());
console.log(new Date("2024-03-10").toISOString());
// UTC methods stay UTC whatever the zone.
console.log(d.getUTCHours(), d.getUTCDate(), Date.UTC(2024, 0, 15, 10, 30), new Date(Date.UTC(2024, 0, 1)).getUTCDay());
const u = new Date(Date.UTC(2024, 0, 15, 22));
u.setUTCHours(25);
console.log(u.toISOString(), u.getDate(), u.getHours());
// A wall-clock time the zone skips reads with the offset before the change,
// one it repeats as its earlier instant.
const gap = new Date(2024, 2, 10, 2, 30);
const repeat = new Date(2024, 10, 3, 1, 30);
console.log(gap.toISOString(), gap.getHours(), repeat.toISOString(), repeat.getHours());
const crossing = new Date(2024, 2, 9, 12);
crossing.setDate(crossing.getDate() + 1);
console.log(crossing.toISOString(), crossing.getHours(), crossing.getTimezoneOffset());
const back = new Date(String(d));
console.log(back.getTime() === Math.floor(d.getTime() / 1000) * 1000, new Date("2024/03/10").toISOString());
const invalid = new Date("not a date");
console.log(invalid.toString(), invalid.toDateString(), invalid.toLocaleString(), invalid.getHours(), invalid.getTimezoneOffset());
const evening = new Date(2024, 11, 31, 23, 5, 9);
console.log(evening.toLocaleTimeString(), new Date(2024, 0, 1, 0, 0, 1).toLocaleTimeString(), new Date(2024, 0, 1, 12, 0).toLocaleTimeString());
console.log(`${evening}`);
const e = new Date(2024, 1, 2, 3, 4, 5); console.log("at " + e, `${e}!`);
