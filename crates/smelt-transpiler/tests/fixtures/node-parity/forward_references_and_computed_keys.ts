// A nested function called before its declaration, and object literals whose
// keys are computed from members.

function repository() {
  function create(params: Record<string, unknown>) {
    return publish({ ...params, documentId: "doc" }).entries[0];
  }

  function publish(opts = {} as any) {
    return { entries: [opts] };
  }

  return { create, publish };
}

const entry: any = repository().create({ id: 7 });
console.log(entry.id, entry.documentId);

const arrowRepository = () => {
  function first(): number {
    return second() + 1;
  }

  function second(): number {
    return 41;
  }

  return first;
};

console.log(arrowRepository()());

const Keys = { a: "alpha", b: "beta" } as const;
const computed = { [Keys.a]: 1, [Keys.b]: 2 };
console.log(computed.alpha, computed.beta, Object.keys(computed).join(","));
