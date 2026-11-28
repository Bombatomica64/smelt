const values: number[] = [1234567.891, 0.5, -1234.5678, 1000, 999, 12345, 0.0004, 2.0005, 1e21, -0];
for (const v of values) {
  console.log(v.toLocaleString(), v.toLocaleString("en-US"), v.toLocaleString("de-DE"), v.toLocaleString("fr-FR"), v.toLocaleString("es-ES"), v.toLocaleString("en-IN"), v.toLocaleString("ja-JP"));
}
console.log((NaN).toLocaleString(), (Infinity).toLocaleString(), (42).toLocaleString("pl-PL"), (12345).toLocaleString("pl-PL"));
