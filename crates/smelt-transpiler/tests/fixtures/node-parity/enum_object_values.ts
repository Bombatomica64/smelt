// An enum read as a value is the object TypeScript emits for it: its members,
// plus a reverse entry for each numeric member, in JavaScript's own-key order.
enum Status { Active = "ACTIVE", Pending = "PENDING", Closed = "CLOSED" }
enum Level { Low, Mid = 5, High }
enum Twice { A = 1, B = 1 }

console.log(Object.values(Status));
console.log(Object.keys(Status).join(","));
console.log(Object.entries(Status).map(([k, v]) => `${k}=${v}`).join("&"));
console.log(Object.values(Status).includes("PENDING" as Status));
const lookup = (s: string): Status | undefined => Object.values(Status).find((v) => v === s);
console.log(lookup("CLOSED"), lookup("nope"));
console.log(Object.keys(Level));
console.log(Object.values(Level).filter((v) => typeof v === "number"));
console.log(Object.keys(Twice), Object.values(Twice));
const statuses = Status;
console.log(statuses.Pending, Object.keys(statuses).length);
