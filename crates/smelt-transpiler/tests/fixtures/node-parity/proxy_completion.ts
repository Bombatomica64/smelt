// The rest of a Proxy's observable behaviour: deleteProperty, a bare read, and
// enumeration following later direct writes to the target.
const log: string[] = [];
const target: any = { a: 1, b: 2 };
const p: any = new Proxy(target, {
  get(t: any, k: any) {
    log.push("get:" + String(k));
    return t[k];
  },
  deleteProperty(t: any, k: any) {
    log.push("delete:" + String(k));
    delete t[k];
    return true;
  },
});
p.a;
p["b"];
console.log(log);
delete p.a;
console.log(log, Object.keys(target));
target.c = 3;
console.log(Object.keys(p), Object.entries(p));
const plain: any = new Proxy({ x: 1 } as any, { get: (t: any, k: any) => t[k] });
delete plain.x;
console.log(Object.keys(plain), "x" in plain);
const refusing: any = new Proxy({} as any, {
  deleteProperty: () => false,
});
console.log(delete refusing.anything);
