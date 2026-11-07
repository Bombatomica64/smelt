function viaField(value: any): string {
  try {
    return String(value.handler(1));
  } catch (error) {
    return (error as Error).name;
  }
}
function viaValue(value: any): string {
  try {
    return String(value(1));
  } catch (error) {
    return (error as Error).name;
  }
}
console.log(viaField({ handler: (n: number) => n + 1 }));
console.log(viaField({}), viaField({ handler: 3 }));
console.log(viaValue((n: number) => n + 2), viaValue(3), viaValue({}));
