// Structural assignability: a value whose type exposes every member a target
// record type declares converts to that record, by reading each target field
// off the source with the same member-read rule `x.member` uses.
//
// A `Response` IS assignable to this `Init` — it has `status`, `statusText` and
// `ok` — and nothing here is an overload or a union. Smelt required nominal
// identity, so the conversion did not exist and the generated crate did not
// compile.
//
// The reads are not field reads: a `Response` keeps its data behind the prelude
// struct's accessors, so the conversion needs a member read spelled from TEXT
// plus a type rather than from a MIR place. `label` is the other half of the
// rule: an OPTIONAL target field the source does not expose is simply absent.

interface Init {
  status?: number
  statusText?: string
  ok?: boolean
  label?: string
}

function describe(init: Init): string {
  return `${init.status ?? 0}/${init.statusText ?? '-'}/${init.ok ?? false}/${init.label ?? 'none'}`
}

function describeMaybe(init?: Init): string {
  if (init === undefined) {
    return 'absent'
  }
  return describe(init)
}

const created = new Response('hi', { status: 201, statusText: 'Created' })
const failed = new Response('no', { status: 500, statusText: 'Boom' })

console.log(describe(created))
console.log(describeMaybe(failed))
console.log(describeMaybe(undefined))
console.log(describe({ status: 7, label: 'plain' }))
