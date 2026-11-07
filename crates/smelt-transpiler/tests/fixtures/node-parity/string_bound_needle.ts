const isText = (value: unknown): value is string => typeof value === "string";
const contains = <const needle extends string>(needle: needle) => (value: unknown) =>
  isText(value) && value.includes(needle);
const opens = <const prefix extends string>(prefix: prefix) => (value: unknown) =>
  isText(value) && value.startsWith(prefix);
console.log(contains("ll")("hello"), contains("zz")("hello"), contains("a")(3));
console.log(opens("he")("hello"), opens("lo")("hello"));
