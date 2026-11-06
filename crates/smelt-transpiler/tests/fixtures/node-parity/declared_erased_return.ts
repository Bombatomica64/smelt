// A declared `any`/`unknown` return is the closure's answer: `null` stays `null`
// rather than being inferred away as `void`.

function apply(value: any, customize: (v: any) => any): any {
  const custom = customize(value);
  if (custom !== undefined) {
    return custom;
  }
  return value;
}
const nullify = (v: any): any => {
  return null;
};
console.log(apply("gone", nullify));
const unk = (v: any): unknown => { return null; };
console.log(apply("gone", unk));
function fx(v: any): any { return null; }
console.log(apply("gone", fx));
const fe = function (v: any): any { return null; };
console.log(apply("gone", fe));
const ex = (v: any): any => null;
console.log(apply("gone", ex));
const six = (v: any): any => { if (v) { return 1; } return "s"; };
console.log(apply(0, six), apply(1, six));
