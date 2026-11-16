// `await v` of a value that is not a promise still suspends for one job turn:
// the jobs queued before it run first, and the caller's synchronous code runs
// before the awaiting function resumes.
console.log("1 sync");
Promise.resolve().then(() => console.log("3 first job"));
(async () => {
  console.log("2 async prefix");
  await null;
  console.log("4 after await null");
  const n: number = await 5;
  console.log("6 after await 5", n);
})();
Promise.resolve().then(() => console.log("5 second job")).then(() => console.log("7 chained job"));
console.log("2b sync end");
