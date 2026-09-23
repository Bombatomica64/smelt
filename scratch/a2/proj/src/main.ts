type Next = () => Promise<void>;
type Handler = (c: string, next: Next) => Promise<void>;
export const requestId = (prefix: string): Handler => {
  return async function requestId(c, next) {
    console.log(prefix + c);
    await next();
  };
};
const countdown = function tick(n: number): number {
  return n <= 0 ? 0 : 1 + tick(n - 1);
};
async function run() {
  await requestId("id-")("x", async () => {
    console.log("next");
  });
  console.log(countdown(3));
}
run();
