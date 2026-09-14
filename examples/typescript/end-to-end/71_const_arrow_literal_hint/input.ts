// An object literal written inline against a CLOSURE's parameter gets the
// parameter's type.
//
// A named function's arguments were lowered with the parameter type as a hint,
// so `showFn({ status: 8 })` built the struct directly. The closure-call path
// lowered its arguments with NO hint, so the identical literal against a
// const-bound arrow was built as a record of erased values and only then
// converted to the struct — every value erased on the way in, at every
// struct-typed argument written inline against an arrow, whatever the struct
// is. The parameter type was already in hand at that call site; it just was not
// passed down.
//
// A hand-written Rust port writes `Opts { status: Some(8.0), .. }` at all three
// of these call sites, so all three must lower the same way. The golden is the
// assertion: `expected.rs` holds three `Opts { .. }` literals and no
// intermediate record, and the HIR golden types each literal as `Opts` rather
// than as a `Dict`.

interface Opts {
  status?: number;
  label?: string;
}

// (1) A plain named function: hinted before and after.
function showFn(opts: Opts): string {
  return `fn ${opts.status ?? 0} ${opts.label ?? "-"}`;
}

// (2) A const-bound arrow with an OPTIONAL parameter. The hint is
// `Optional<Opts>`, and the literal has to see through the option to the
// struct.
const showOptional = (opts?: Opts): string =>
  `optional ${opts?.status ?? 0} ${opts?.label ?? "-"}`;

// (3) A const-bound arrow with a REQUIRED parameter: the shape the note was
// filed against.
const showRequired = (opts: Opts): string =>
  `required ${opts.status ?? 0} ${opts.label ?? "-"}`;

console.log(showFn({ status: 8, label: "fn" }));
console.log(showOptional({ status: 7, label: "opt" }));
console.log(showRequired({ status: 9, label: "req" }));

// A partial literal still fills the absent keys with `None` rather than
// erasing, and an omitted optional argument is still absent.
console.log(showRequired({ label: "partial" }));
console.log(showOptional());

// An array literal argument takes the parameter's element type the same way,
// so the elements are structs rather than erased records.
const total = (items: Opts[]): number => {
  let sum = 0;
  for (const item of items) {
    sum += item.status ?? 0;
  }
  return sum;
};

console.log(total([{ status: 1 }, { status: 2, label: "two" }]));
