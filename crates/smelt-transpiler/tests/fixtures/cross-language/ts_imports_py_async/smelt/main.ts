import { double } from "./work";
async function main(): Promise<void> {
  console.log(await double(21));
}
main();
