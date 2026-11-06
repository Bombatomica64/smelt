class Direct extends DOMException { constructor(m = 'dflt') { super(m); } }
function catchIt(f: () => void): string {
  try { f(); return 'none'; } catch (e) {
    return [e instanceof Error, e instanceof DOMException, e instanceof Direct].join(',');
  }
}
console.log(catchIt(() => { throw new Direct(); }));
console.log(catchIt(() => { throw new DOMException('a'); }));
