// A class may replace one of its own methods at runtime. The reassigned method
// is carried as a function-typed field so the assignment is expressible, while
// a sibling method that is never assigned stays an ordinary method.
class Router {
  log: string = "";

  pick(path: string): string {
    return "slow:" + path;
  }

  describe(path: string): string {
    return "[" + path + "]";
  }

  route(path: string): string {
    const before = this.pick(path);
    // Specialize: later calls skip the slow path entirely.
    this.pick = (value: string) => "fast:" + value;
    const after = this.pick(path);
    this.log = this.log + this.describe(path);
    return before + "," + after;
  }
}

const router = new Router();
const first = router.route("a");
const second = router.route("b");
console.log(first);
console.log(second);
console.log(router.log);
