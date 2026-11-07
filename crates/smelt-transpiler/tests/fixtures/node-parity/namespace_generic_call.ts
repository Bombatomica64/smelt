import * as _ from "./lib.ts";

const total = _.iterate(3, (value, index) => value + index, 10);
console.log(total + 1, total.toFixed(1));
const text = _.iterate(2, (value, index) => value + "-" + index, "a");
console.log(text.toUpperCase(), text.length);
const head = _.first(["x", "y"]);
console.log(head?.toUpperCase());
console.log(_.wrap(2).value * 3, _.wrap("s").value.length);
