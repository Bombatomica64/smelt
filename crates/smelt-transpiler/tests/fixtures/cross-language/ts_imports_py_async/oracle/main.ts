import { double } from "./work.ts";
async function main(): Promise<void> {
  console.log(await double(21));
}
main();
