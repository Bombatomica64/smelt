import { check } from "./guard.ts";
try {
  console.log(check(1));
  console.log(check(-2));
} catch (e) {
  console.log("caught", e instanceof Error, (e as Error).message);
}
