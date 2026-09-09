// An `Error` subclass forwarding an OPTIONAL message to `super`.
//
// `Error.message` is a `string` and `new Error(undefined).message` is `""`, so
// `super(options?.message)` is a defaulting, not a narrowing assertion: the
// callee's spec supplies the value for an absent argument. The `Error` base
// super-call lowering now coalesces the argument with the slot's own spec
// default (`lowering::decls::super_call`), which is the same default the
// absent-argument path already wrote. Before that, the assignment fell through
// to the emitter's optional-to-required coercion and the generated program
// panicked with "optional value was absent after narrowing" for every subclass
// constructed without a message — Hono's `HTTPException` among them.
interface HTTPExceptionOptions {
  message?: string;
}

class HTTPException extends Error {
  readonly status: number;

  constructor(status: number, options?: HTTPExceptionOptions) {
    super(options?.message);
    this.status = status;
  }
}

const described = new HTTPException(404, { message: "not found" });
console.log(described.status, described.message, described.name);

const bare = new HTTPException(500);
console.log(bare.status, JSON.stringify(bare.message));

// The same shape one level deeper: the derived constructor forwards its own
// optional message on, so the defaulting has to survive a declared base class
// as well as the host `Error` base.
class GatewayError extends HTTPException {
  constructor(options?: HTTPExceptionOptions) {
    super(502, options);
  }
}

const gateway = new GatewayError();
console.log(gateway.status, JSON.stringify(gateway.message));
