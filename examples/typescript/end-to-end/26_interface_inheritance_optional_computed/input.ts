interface Entity {
  ["id"]: string;
}

interface Named extends Entity {
  name?: string;
}

class User implements Named {
  ["id"]: string;

  constructor(id: string) {
    this.id = id;
  }
}

const user = new User("u1");
console.log(user.id);
