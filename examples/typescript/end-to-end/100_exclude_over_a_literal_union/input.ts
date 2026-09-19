// `Exclude<A, B>` removes from `A` the arms `B` names.
//
// Two shapes, both general:
//
//  * `StatusCode` is a union of NUMERIC LITERAL types. Every arm's widened type
//    is `number`, so the union itself is `number`, and removing some of its
//    literals still leaves a union of literals — still `number`. The excluded
//    field must stay arithmetic, not a runtime-tagged value.
//  * `Named` has arms Smelt models distinctly, so `Exclude` really does drop
//    the arm it names and the remainder narrows by `typeof`.

type StatusCode = 200 | 204 | 301 | 404 | 500
type ContentlessStatusCode = 204 | 304
type ContentfulStatusCode = Exclude<StatusCode, ContentlessStatusCode>

type Named = string | number | boolean
type WithoutNumber = Exclude<Named, number>

class Reply {
  readonly status: ContentfulStatusCode

  constructor(status: ContentfulStatusCode) {
    this.status = status
  }

  describe(): string {
    return `status=${this.status + 1}`
  }
}

function render(value: WithoutNumber): string {
  if (typeof value === 'string') {
    return `s:${value}`
  }
  return `b:${value}`
}

const reply = new Reply(404)
console.log(reply.describe())
console.log(render('hi'))
console.log(render(true))
