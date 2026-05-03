class User {
  public first: string;
  public last: string;

  constructor(first: string, last: string) {
    this.first = first;
    this.last = last;
  }

  label(): string {
    return this.first + this.last;
  }
}
const user = new User("Ada", "Lovelace");
console.log(user.label());
