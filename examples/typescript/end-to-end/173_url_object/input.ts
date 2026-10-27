// A WHATWG `URL` is a mutable reference object: component writes are seen
// through every alias, and `searchParams` is a live view of the query.

function trimTrailingSlash(input: string): string {
  const url = new URL(input);
  url.pathname = url.pathname.substring(0, url.pathname.length - 1);
  return url.toString();
}

function appendSlash(url: URL): URL {
  url.pathname += "/";
  return url;
}

const url = new URL("https://user:pw@example.com:8080/a/b/?x=1&y=2#frag");
console.log(url.href);
console.log(url.origin, url.protocol, url.username, url.password);
console.log(url.host, url.hostname, url.port, url.pathname, url.search, url.hash);

function moveThroughAlias(target: URL): void {
  const alias = target;
  alias.pathname = "/moved";
  alias.hash = "";
}
moveThroughAlias(url);
console.log(url.href);

url.searchParams.append("z", "3");
url.searchParams.delete("x");
console.log(url.search, url.searchParams.get("y"));

url.search = "?q=hello+world";
console.log(url.searchParams.get("q"), url.searchParams.has("y"));

url.port = "443";
url.protocol = "http:";
console.log(url.toString());
console.log(JSON.stringify({ location: url.toJSON() }));

console.log(trimTrailingSlash("https://example.com/the/endpoint/?k=v"));
console.log(appendSlash(new URL("https://example.com/dir")).pathname);

const relative = new URL("../c?d=1", "https://example.com/a/b/");
console.log(relative.href);
console.log(new URL(relative).pathname);
console.log(new Request(relative).url);
console.log(`${relative}`);

try {
  new URL("not a url");
  console.log("parsed");
} catch (error) {
  if (error instanceof TypeError) {
    console.log("TypeError", error.message);
  }
}

try {
  url.href = "::";
  console.log("assigned");
} catch (error) {
  console.log(error instanceof TypeError, url.pathname);
}
