class Registry {
  publicLog: string = "";
  privateLog: string = "";

  private stash(value: string): void {
    this.privateLog = this.privateLog + value;
  }

  keep(value: string): void {
    this.publicLog = this.publicLog + value;
  }

  add(value: string): void {
    this.stash(value);
    this.keep(value);
  }
}
const registry = new Registry();
registry.add("a");
registry.add("b");
console.log(registry.privateLog + "," + registry.publicLog);
