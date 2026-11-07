type Matcher<input> = { test: (value: input) => boolean };
export type StringChainable<p extends Matcher<string>> = p & {
  startsWith<prefix extends string>(prefix: prefix): StringChainable<Matcher<string>>;
  endsWith<suffix extends string>(suffix: suffix): StringChainable<Matcher<string>>;
};
function chain(test: (value: string) => boolean): StringChainable<Matcher<string>> {
  return {
    test,
    startsWith: (prefix) => chain((value) => test(value) && value.startsWith(prefix)),
    endsWith: (suffix) => chain((value) => test(value) && value.endsWith(suffix)),
  } as StringChainable<Matcher<string>>;
}
export const string = chain((value) => typeof value === "string");
