// `replace` substitutes ToString of what the replacer answers.
const nothing: any = undefined;
console.log("a-b".replace(/-/, () => nothing));
console.log("a-b".replace(/-/g, () => null as any));
console.log("a1".replace(/\d/, () => 5 as any));
console.log("a1".replace(/\d/, () => true as any));
console.log("a1".replace(/\d/, (m) => m + m));
console.log("a1".replace(/\d/, () => [1, 2] as any));
