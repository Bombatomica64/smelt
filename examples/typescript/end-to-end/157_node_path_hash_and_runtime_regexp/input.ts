// Three host features as general rules, every line diffed against Node.
//
// * `node:path` (POSIX): `join`/`resolve`/`normalize`/`dirname`/`basename`/
//   `extname`/`relative`/`isAbsolute`/`sep` are Node's own algorithms, and the
//   function is found from the IMPORT the callee names — a default import, a
//   namespace import through `path.posix`, and an aliased named import from
//   `node:path/posix` are one module. A program's own `join` is untouched.
// * `node:crypto` `createHash`: a stateful `Hash` whose `update` chains on one
//   hasher and whose `digest(encoding)` finalizes it; an unknown algorithm and
//   a second `digest` throw Node's catchable errors.
// * `new RegExp(pattern)` over a RUN-TIME pattern compiles at construction and
//   throws a catchable `SyntaxError` for a malformed pattern (duplicate group
//   names included); a literal pattern keeps the infallible construction.
import path from 'node:path'
import * as nsPath from 'path'
import { join as posixJoin } from 'node:path/posix'
import { basename, dirname, extname, isAbsolute, normalize, relative, sep } from 'node:path'
import { createHash } from 'node:crypto'

function ownJoin(first: string, second: string): string {
  return `${first}+${second}`
}

const segments: string[] = ['public', 'sub/', '../file.html']
console.log(path.join('/home/app', 'static//main.html'))
console.log(posixJoin(...segments), nsPath.posix.join('a/b', '..', 'c'))
console.log(path.join(), posixJoin(''), posixJoin('public', ''), posixJoin('./x', '/abs'))
console.log(path.resolve('/a/b', './c', '../d'), path.resolve('/x', '/y', 'z'))
console.log(normalize('/a//b/../c/.'), normalize('a/..'), normalize(''), normalize('x/'))
console.log(dirname('/a/b/c.txt'), dirname('a'), dirname('/'), dirname('//x'))
console.log(basename('/a/b/c.txt'), basename('/a/b/c.txt', '.txt'), basename('a/b/'))
console.log(extname('x.tar.gz'), extname('.hidden'), extname('a.'), extname('noext'))
console.log(relative('/a/b/c', '/a/d'), relative('/a', '/a/b/c'), relative('/same', '/same') === '')
console.log(isAbsolute('/x'), isAbsolute('x'), sep, path.sep)
console.log(ownJoin('a', 'b'))

console.log(createHash('sha256').update('hello').digest('hex'))
console.log(createHash('SHA1').update('hel').update('lo').digest('base64'))
console.log(createHash('md5').update('68656c6c6f', 'hex').digest('hex'))
console.log(createHash('sha512').update(new Uint8Array(3)).digest('base64url'))

function hexOf(algorithm: string, data: string): string {
  try {
    return createHash(algorithm).update(data).digest('hex')
  } catch (e) {
    return 'unsupported'
  }
}
console.log(hexOf('sha384', ''), hexOf('bogus', 'x'))

function digestTwice(): string {
  const hash = createHash('sha1')
  hash.update('once')
  const first = hash.digest('hex')
  try {
    hash.digest('hex')
    return 'no throw'
  } catch (e) {
    return `${first.length} then threw`
  }
}
console.log(digestTwice())

class UnsupportedPathError extends Error {}

function compileRoute(route: string): RegExp {
  try {
    return new RegExp(`^${route}$`)
  } catch {
    throw new UnsupportedPathError()
  }
}

function describeCompile(route: string): string {
  try {
    const compiled = compileRoute(route)
    return `${compiled.source} ${compiled.test('/7/8')}`
  } catch (e) {
    return `rejected ${e instanceof UnsupportedPathError}`
  }
}

console.log(describeCompile('/(?<id>[^/]+)/(?<rest>[^/]+)'))
console.log(describeCompile('/(?<id>[^/]+)/(?<id>[^/]+)'))
console.log(describeCompile('/(unclosed'))

function tryFlags(flags: string): string {
  try {
    return new RegExp('a', flags).flags
  } catch {
    return 'bad flags'
  }
}
console.log(tryFlags('gi'), tryFlags('gg'), tryFlags('q'))
const literal = new RegExp('x+', 'g')
console.log(literal.source, literal.flags)
