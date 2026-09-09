const encoder = new TextEncoder();
const bytes = encoder.encode("héllo");
console.log(bytes.length);
console.log(bytes.byteLength);
console.log(encoder.encoding);

const decoder = new TextDecoder("UTF8");
console.log(decoder.decode(bytes));
console.log(decoder.encoding);

console.log(new TextEncoder().encode("").length);
console.log(new TextDecoder().decode(new TextEncoder().encode("round trip")));

const emoji = new TextEncoder().encode("😀");
console.log(emoji.length);
console.log(new TextDecoder().decode(emoji));
