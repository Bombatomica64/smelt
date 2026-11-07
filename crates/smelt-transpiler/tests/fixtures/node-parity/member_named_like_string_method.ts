import * as P from "./pattern.ts";

const greeting = P.text.startsWith("hello").includes("!");
console.log(greeting.test("hello there!"), greeting.test("hello there"), greeting.test("hi!"));
