# Why smelt exists

Smelt started with a production incident, like most strong opinions do.

## The incident

I was working on a Django backend with an Angular frontend. One day another
developer changed a field's type in the API — `number` became `str`. Nothing
complained. Not the backend, not the frontend, not CI. The type annotations
were right there in both codebases, and they disagreed, and nothing on Earth
cared.

I found out when one of my own pushes went to production and broke a lot of
stuff that had nothing to do with my change. The contract had been broken for
a while; my deploy just happened to be the one standing closest when the bill
came due.

## The conviction

The lesson I chose to take from this was not "add Pydantic" or "add Zod" —
bolting a runtime validator onto the boundary means admitting your types are
decorative and paying CPU cycles to re-check what the compiler should have
known. The lesson I took was: **my types should always be real.** If a
function says it takes a `number`, that should be a fact about the program,
not a suggestion.

(We later moved to Zod in the frontend anyway. Hahaha. The industry pulls you
back in. But the conviction stuck.)

## The compiler

So I started studying compilers, and the shape fell out of the conviction:

- **Per-language frontends** that take strictly-typed TypeScript and Python
  and normalize them.
- **HIR** — a typed, language-agnostic AST both frontends lower into.
- **MIR** — an SSA-style middle representation where ownership, closures,
  and control flow get made explicit.
- **Rust codegen** at the bottom, where a type is as real as types get.

One early decision did a lot of quiet work: **the parsing and the type
information come from established libraries, not from me.** Oxc parses the
TypeScript; Ruff parses the Python; the type checkers that already won those
ecosystems define what the types mean. I did not want to be the person whose
hand-rolled parser had an opinion about optional chaining. If the goal is
types you can't accidentally break, step one is not writing a parser you can
accidentally break.

## The accident

Then, after a while, the design paid out something I hadn't planned.

Both frontends lower into the *same* HIR. The generated crate doesn't know or
care which language a module came from — in the end, everything is Rust. Which
means a Python file can import from a TypeScript file, call its functions,
and the whole thing type-checks and compiles as one crate:

```python
from math import add        # math is math.ts

result: float = add(2.0, 3.0)
```

The exact failure that started this project — two languages disagreeing about
one contract — is not detected here. It is *unrepresentable*. There is one
contract, in one IR, checked by one compiler, and smelt writes `.d.ts` and
`.pyi` stubs next to every module so your editor sees it too.

## The rest

The rest is just running every library I can get my hands on through it and
fixing whatever breaks. Remeda's test suite passes as native Rust tests —
1,789 of them. es-toolkit is the current campaign. Each library is a few
hundred small arguments with reality, and reality keeps winning them, and the
compiler keeps getting better.

That's the project: types that are real, enforced by making them Rust.
