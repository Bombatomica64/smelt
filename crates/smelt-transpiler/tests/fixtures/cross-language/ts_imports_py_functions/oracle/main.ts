import { add, greet, scale, pad } from "./mathlib.ts";
console.log(add(2, 3), greet("Ada"), scale([1.5, 2], 2).join(","), pad("x"), pad("y", 3));
