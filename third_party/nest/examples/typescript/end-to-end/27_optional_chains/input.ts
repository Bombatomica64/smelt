class User {
  name: string;
  scores: number[];

  constructor(name: string, scores: number[]) {
    this.name = name;
    this.scores = scores;
  }

  label(): string {
    return "user";
  }
}

const present: User | null = new User("Ada", [3]);
const missing: User | null = null;

const name = present?.name;
const absentName = missing?.name;
const score = present?.scores?.[0];
const label = present?.label();

console.log(name);
console.log(absentName);
console.log(score);
console.log(label);
