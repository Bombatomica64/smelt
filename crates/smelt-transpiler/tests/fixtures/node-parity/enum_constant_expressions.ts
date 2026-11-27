// Enum members initialised from earlier members, bare (`A | B`) or qualified
// (`F.A << 3`), through the bitwise and arithmetic operators TypeScript folds.
enum F {
  A = 1,
  B = 2,
  AB = A | B,
  C = F.A << 3,
  D = ~A,
  E = AB & B,
  G = AB ^ 1,
  H = -1 >>> 28,
  I = (B + 1) * 2,
  J = -8 >> 1,
}
console.log(F.AB, F.C, F.D, F.E, F.G, F.H, F.I, F.J);
console.log(F[3], F.A | F.B);
function has(flags: F, bit: F): boolean {
  return (flags & bit) !== 0;
}
console.log(has(F.AB, F.B), has(F.A, F.B));
const enum G {
  X = 4,
  Y = X * 2,
  Z = G.Y | X,
}
console.log(G.Y, G.Z);
