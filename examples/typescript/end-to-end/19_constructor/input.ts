class Box {
  public value: string;

  constructor(value: string) {
    this.value = value;
  }
}
const box = new Box("ok");
console.log(box.value);
