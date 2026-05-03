interface Named {
  name: string;
}
class User implements Named {
  public name: string;

  constructor(name: string) {
    this.name = name;
  }
}
const user = new User("Ada");
console.log(user.name);
