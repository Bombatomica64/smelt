// A type alias that NAMES a class is that class. Recording the class's fields
// as the alias's own made a reference lower to a nominal record named after
// the alias, which nothing backs, so the value erased (Hono's
// `type IdContext = Context<Env, '/:id'>` handlers saw `c` as `unknown`).
class GCtx<P extends string = string> {
  path: P;
  constructor(path: P) {
    this.path = path;
  }
  text(value: string): string {
    return value + '!';
  }
}

type GAlias = GCtx;
type IdAlias = GCtx<'/:id'>;

function viaAlias(c: GAlias): string {
  return c.text('alias');
}

function viaIdAlias(c: IdAlias): string {
  return c.text('id');
}

console.log(viaAlias(new GCtx('/b')), viaIdAlias(new GCtx('/:id')));
