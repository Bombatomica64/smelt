// A path trie in the shape of Hono's reg-exp router: keyed reads that the
// code itself tests for presence, a symbol sentinel thrown through an
// optional-chained call inside `try`, and a rethrown `Error` subclass.
const PATH_ERROR = Symbol();

class UnsupportedPathError extends Error {}

class TrieNode {
  children: Record<string, TrieNode> = {};
  terminal: boolean = false;

  insert(tokens: string[]): void {
    let node: TrieNode = this;
    for (const token of tokens) {
      let next: TrieNode;
      next = node.children[token];
      if (!next) {
        for (const key in node.children) {
          // a label and a literal cannot share a parent
          if (key.startsWith(':') !== token.startsWith(':')) {
            throw PATH_ERROR;
          }
        }
        next = node.children[token] = new TrieNode();
      }
      node = next;
    }
    if (node.terminal) {
      throw PATH_ERROR;
    }
    node.terminal = true;
  }
}

class Router {
  tries: Record<string, TrieNode> = { ALL: new TrieNode() };
  routes: Record<string, Record<string, string[]>> = { ALL: {} };
  log: string[] = [];

  #insertPath(method: string, path: string): void {
    try {
      this.tries[method]?.insert(path.split('/').filter((part) => part !== ''));
    } catch (e) {
      if (e === PATH_ERROR) {
        throw new UnsupportedPathError(path);
      }
      throw e;
    }
  }

  add(method: string, path: string, handler: string): void {
    if (!this.routes[method]) {
      this.tries[method] = new TrieNode();
      this.routes[method] = {};
      for (const p in this.routes['ALL']) {
        this.routes[method][p] = [...this.routes['ALL'][p]];
        this.#insertPath(method, p);
      }
    }
    if (/\((?!\?:)/.test(path)) {
      throw new UnsupportedPathError(path);
    }
    if (!this.routes[method][path]) {
      this.#insertPath(method, path);
      this.routes[method][path] = [];
    }
    this.routes[method][path].push(handler);
    this.log.push(method + ' ' + path);
  }
}

function tryAdd(router: Router, method: string, path: string): void {
  try {
    router.add(method, path, 'h:' + path);
    console.log('added', method, path);
  } catch (e) {
    if (e instanceof UnsupportedPathError) {
      console.log('rejected', path, e instanceof Error, `${e.name}:${e.message}`);
    } else {
      console.log('unexpected', path);
    }
  }
}

const router = new Router();
tryAdd(router, 'GET', '/posts/:id');
tryAdd(router, 'GET', '/posts/:id');
tryAdd(router, 'GET', '/posts/new');
tryAdd(router, 'POST', '/posts');
tryAdd(router, 'GET', '/users/(a|b)');
console.log(router.log.join(', '));
console.log(JSON.stringify(router.routes));
