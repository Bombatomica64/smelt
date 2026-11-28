// `splice` on an array held in a field edits that array in place and returns
// the removed items, as it does on a local.
class Inventory {
  private stock: string[] = ["hammer", "apple", "saw", "pear"];
  readonly removed: string[][] = [];
  sell(name: string): boolean {
    const index = this.stock.indexOf(name);
    if (index < 0) return false;
    this.removed.push(this.stock.splice(index, 1));
    return true;
  }
  insertAt(index: number, ...names: string[]): void { this.stock.splice(index, 0, ...names); }
  get report(): string { return this.stock.join(","); }
}
const inv = new Inventory();
console.log(inv.sell("apple"), inv.sell("ghost"), inv.report);
inv.insertAt(1, "nail", "glue");
console.log(inv.report, inv.removed);
const holder = { items: [1, 2, 3, 4, 5] };
console.log(holder.items.splice(-2), holder.items);
