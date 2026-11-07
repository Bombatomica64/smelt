import * as P from "./pattern.ts";

const greeting = P.string.startsWith("hello ").endsWith("!");
console.log(greeting.test("hello there!"), greeting.test("hello there"), greeting.test("hi!"));
