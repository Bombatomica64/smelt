import { tz } from "@date-fns/tz";

const inTokyo = tz("Asia/Tokyo");
const source = new Date("2024-01-01T18:07:08.009Z");
const zoned = inTokyo(source);
console.log(zoned.getTime(), zoned.getDate(), zoned.getDay(), zoned.getHours(), zoned.getUTCHours());
console.log(zoned.getTimezoneOffset(), zoned.toISOString());
console.log(zoned.valueOf(), zoned.toString(), zoned.toLocaleTimeString());
console.log(new Date(zoned).toISOString());

function describe(date: Date): void {
  console.log(date.valueOf(), date.toString());
  console.log(date.toDateString(), date.toTimeString());
  console.log(date.toLocaleDateString(), date.toLocaleTimeString(), date.toLocaleString());
}

const tokyo: Date = zoned;
describe(tokyo);
console.log(JSON.stringify(zoned), JSON.stringify(tokyo));
console.log(inTokyo("2024-01-02T03:07:08.009").toISOString());
tokyo.setHours(12, 30);
tokyo.setDate(4);
console.log(zoned.toISOString(), tokyo.getTime(), source.toISOString());
tokyo.setUTCHours(17);
console.log(tokyo.toISOString(), tokyo.toUTCString());

const inNewYork = tz("America/New_York");
const first = inNewYork("2024-11-03T05:30:00Z");
const second = inNewYork("2024-11-03T06:30:00Z");
console.log(first.toISOString(), second.toISOString(), second.getTime() - first.getTime());
const before = inNewYork("2024-03-09T17:00:00Z");
before.setDate(10);
console.log(before.toISOString(), before.getTimezoneOffset());
