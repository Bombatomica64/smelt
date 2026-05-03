interface Named {
  name: string;
  label(prefix: string): string;
}
class User implements Named {
  public name: string;

  constructor(name: string) {
    this.name = name;
  }

  label(prefix: string): string {
    return prefix + this.name;
  }
}
const user = new User("Ada");
console.log(user.label("Hi "));
