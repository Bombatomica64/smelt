export class Counter {
  count: number;
  stepSize: number;
  constructor(start: number) { this.count = start; this.stepSize = 1; }
  inc(): number { this.count += this.stepSize; return this.count; }
  bumpBy(n: number): number { this.count += n; return this.count; }
}
