let i = 0;
let sum = 0;
while (i < 6) {
  i = i + 1;
  if (i === 2) {
    continue;
  }
  sum = sum + i;
}
console.log(sum);
