// An async function that can throw, passed as a callback, returns a promise:
// its error rejects that promise instead of escaping the callback.
async function fetchUser(id: number): Promise<{ id: number; name: string }> {
  if (id === 3) throw new Error(`user ${id} failed`);
  return { id, name: `user${id}` };
}
async function main(): Promise<void> {
  const users = await Promise.all([1, 2].map(fetchUser));
  console.log(users.map((u) => u.name));
  const settled = await Promise.allSettled([1, 3].map(fetchUser));
  for (const s of settled) {
    if (s.status === "fulfilled") console.log("ok", s.value.name);
    else console.log("err", (s.reason as Error).message);
  }
  const pending = [2, 3].map(fetchUser);
  console.log(pending.length);
  try { await pending[1]; } catch (e) { console.log("caught", (e as Error).message); }
  await pending[0].then((u) => console.log("then", u.id));
}
main().then(() => console.log("end"));
