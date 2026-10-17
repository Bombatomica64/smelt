// `Object.assign(instance, source)` writes onto the instance itself. The
// record lowering used to merge into a COPY of the instance's erased view, so
// the instance never changed: a constructor doing
// `const { strict, ...rest } = options; Object.assign(this, rest)` kept its
// field defaults (Hono's `basePath()` clone kept a default router).
type Options = { strict?: boolean; label?: string; size?: number };

class Crate {
  label = 'none';
  size = 0;
  constructor(options: Options = {}) {
    const { strict, ...rest } = options;
    Object.assign(this, rest);
    if (strict) {
      this.size = -1;
    }
  }
}

const big = new Crate({ label: 'big', size: 3 });
console.log(big.label, big.size);
const sized = new Crate({ size: 5, strict: false });
console.log(sized.label, sized.size);
const target = new Crate();
Object.assign(target, { label: 'later' });
console.log(target.label, target.size);
