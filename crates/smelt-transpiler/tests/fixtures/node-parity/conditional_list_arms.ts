// Ternary arms that are arrays of different element types join; neither arm is
// retyped as the other.

function plain(c: boolean): (string | number)[] {
  const t = c ? ["a"] : [1, 2, 3];
  return t;
}
console.log(plain(true).length, plain(false).length);
const n: number[] = [];
const u1 = n.length === 0 ? [1, 2] : [3];
console.log(u1.length, u1[0] + 1);
const u2 = n.length === 0 ? ["x"] : [null];
console.log(JSON.stringify(u2));
const u3 = n.length > 0 ? [[1]] : [["a"]];
console.log(JSON.stringify(u3));
const u4 = n.length > 0 ? [1] : [];
console.log(u4.length);
const u5 = n.length === 0 ? [{ a: 1 }] : [{ a: 2 }];
console.log(u5[0].a);
