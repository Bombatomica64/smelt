const query = new URLSearchParams("?a=1&b=two&a=3");
query.append("greeting", "hello world");
query.set("b", "TWO");

console.log(query.get("a") ?? "null");
console.log(query.getAll("a").join("|"));
console.log(query.get("A") ?? "null");
console.log(query.has("greeting"), query.has("missing"));
console.log(query.size);
console.log(query.toString());

query.delete("a");
query.sort();
console.log(query.toString());
console.log([...query.keys()].join(","));
for (const [name, value] of query.entries()) {
  console.log(name, value);
}
