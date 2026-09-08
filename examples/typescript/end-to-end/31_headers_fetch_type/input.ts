const request = new Headers({ "Content-Type": "text/plain" });
request.append("Accept", "text/html");
request.append("accept", "application/json");

const response = new Headers(request);
response.set("X-Trace", "first");
response.set("x-trace", "second");
response.append("Set-Cookie", "a=1");
response.append("set-cookie", "b=2");
response.delete("content-type");

console.log(request.get("content-type") ?? "null");
console.log(request.get("accept") ?? "null");
console.log(response.get("x-trace") ?? "null");
console.log(response.get("content-type") ?? "null");
console.log(response.has("set-cookie"), response.has("content-type"));
console.log(response.getSetCookie().join("|"));
console.log([...response.keys()].join(","));
for (const [name, value] of response.entries()) {
  console.log(name, value);
}
