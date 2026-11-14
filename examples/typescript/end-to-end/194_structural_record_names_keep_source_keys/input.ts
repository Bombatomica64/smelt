// Two inline object types that differ only in how their keys are spelled are
// two structs.
//
// An inline object type whose fields differ in type lowers to a struct named
// by its structure (`decls::anonymous_record`). The name hashed the Rust field
// names, and Rust folds `userId` and `user_id` to the same field, so
// `{ userId: number; firstName: string }` and `{ user_id: number;
// first_name: string }` were ONE struct: the returned record was read back
// through the parameter's keys and the program threw
// `expected number, got undefined at ...userId`. The name now hashes each
// field's source spelling, as the keys a JavaScript reader sees.
//
// Every line below is diffed against Node.

function snake(input: { userId: number; firstName: string }): { user_id: number; first_name: string } {
  return { user_id: input.userId, first_name: input.firstName };
}

function camel(input: { user_id: number; first_name: string }): { userId: number; firstName: string } {
  return { userId: input.user_id, firstName: input.first_name };
}

const out = snake({ userId: 7, firstName: "Ada" });
console.log(JSON.stringify(out), out.user_id + 1, out.first_name.length);
const back = camel(out);
console.log(JSON.stringify(back), back.userId * 2);
