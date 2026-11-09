// `export default <expression>` is evaluated where it is written.
const log: string[] = [];

function make(label: string): number {
  log.push(label);
  console.log("evaluated " + label);
  return log.length;
}

console.log("before");
export default make("default");
console.log("after " + String(log.length));
