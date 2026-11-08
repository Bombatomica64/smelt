export class Counter {
  count: number;
  constructor(start: number) { this.count = start; }
  inc(): number { this.count += 1; return this.count; }
}
