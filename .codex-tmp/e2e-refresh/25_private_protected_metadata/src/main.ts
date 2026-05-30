class Secret {
  private token: string;
  protected count: number;

  constructor(token: string, count: number) {
    this.token = token;
    this.count = count;
  }

  reveal(): string {
    return this.token;
  }
}
const secret = new Secret("s", 1);
console.log(secret.reveal());
