// Two classes may declare a method of the same name with different signatures:
// a method's Rust name is unique only inside its own `impl` block. The emitted
// parameter/return type maps were keyed by that bare name, so one class's
// method answered for the other's — and because those maps decide the type a
// call site converts FROM, a call to a `string`-returning method was converted
// from the OTHER class's `void`, which renders a constant and drops the call
// itself (H63).
class Ticker {
  count = 0;

  bump(key: string): void {
    this.count += key.length;
  }
}

class Labeller {
  label = 'labelled';

  // Same name, different return type: this is the one that lost its value.
  bump(key: string): string {
    return this.label + key;
  }

  // Same name, different parameter types: the argument half of the same maps.
  describe(size: number): string {
    return this.label + '/' + String(size);
  }
}

class Sizer {
  describe(text: string): number {
    return text.length;
  }
}

const ticker = new Ticker();
ticker.bump('ab');
console.log('void method: ' + String(ticker.count));

const labeller = new Labeller();
console.log('value method: ' + labeller.bump('!'));
const held = labeller.bump('?');
console.log('bound value: ' + held);
console.log('same name, other params: ' + labeller.describe(3));
console.log('other class: ' + String(new Sizer().describe('abcd')));
