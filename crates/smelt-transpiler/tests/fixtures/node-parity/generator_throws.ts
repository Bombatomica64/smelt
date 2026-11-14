// A throw from a generator body reaches the try around whatever resumed it:
// .next(), for-of, a spread, Array.from, or a yield* in another generator;
// the generator is done afterwards.
function* g(): Generator<number> { yield 1; throw new Error("boom"); }
const it = g();
console.log(it.next().value);
try { it.next(); } catch (e) { console.log("caught", (e as Error).message); }
console.log(it.next().done);
function* h(n: number): Generator<number, string> { for (let i = 0; i < n; i++) { if (i === 2) throw new RangeError("at " + i); yield i; } return "end"; }
function collect(n: number): string { const seen: number[] = []; try { for (const v of h(n)) seen.push(v); return "done " + seen.join(","); } catch (e) { return "caught " + (e as Error).message + " after " + seen.join(","); } }
console.log(collect(2), "|", collect(5));
function* inner(): Generator<number> { yield 1; try { yield 2; } finally { console.log("cleanup"); } }
for (const v of inner()) console.log(v);
const thrower = (function* () { yield "a"; throw "plain"; })();
for (let i = 0; i < 3; i++) { try { const r = thrower.next(); console.log(r.value, r.done); } catch (e) { console.log("caught", e); } }
function* d3(): Generator<number> { yield 1; throw new Error("inner"); }
try { console.log([...d3()]); } catch (e) { console.log("outside", (e as Error).message); }
try { console.log(Array.from(d3())); } catch (e) { console.log("from", (e as Error).message); }
function* ok3(): Generator<number> { yield 1; yield 2; }
const xs: number[] = [...ok3()];
console.log(xs);
function* d(): Generator<number, string> { yield 1; throw new Error("inner"); }
function* outer(): Generator<number> { try { const r = yield* d(); console.log("unreached", r); } catch (e) { console.log("caught", (e as Error).message); yield 99; } }
console.log([...outer()]);
function* plain(): Generator<number> { yield* d(); }
try { console.log([...plain()]); } catch (e) { console.log("outside", (e as Error).message); }
function* fine(): Generator<number, string> { yield 5; return "r"; }
function* wrap(): Generator<number, string> { const r = yield* fine(); yield* [7, 8]; return r + "!"; }
const w = wrap(); console.log(w.next().value, w.next().value, w.next().value, w.next());
