type Shape = { kind: "circle"; r: number } | { kind: "square"; side: number };
function area(s: Shape): number {
  switch (s.kind) {
    case "circle": return Math.round(Math.PI * s.r * s.r);
    case "square": return s.side * s.side;
    default: {
      const unreachable: never = s;
      return unreachable;
    }
  }
}
function describe(s: Shape): string {
  if (s.kind === "circle") return "circle " + s.r;
  if (s.kind === "square") return "square " + s.side;
  const exhaustive: never = s;
  throw new Error("unexpected " + exhaustive);
}
console.log(area({ kind: "circle", r: 2 }), area({ kind: "square", side: 3 }));
console.log(describe({ kind: "circle", r: 1 }), describe({ kind: "square", side: 4 }));
