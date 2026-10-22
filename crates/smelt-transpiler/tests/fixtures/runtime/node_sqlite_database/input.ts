// node:sqlite through a class that owns its database, as an app repository does:
// DDL, positional and named parameters, every column kind, run() counts, a
// thrown constraint error, a row whose columns change when the schema does
// (the reason rows are erased), a BLOB column (a Uint8Array), and close().
import { DatabaseSync, StatementSync } from 'node:sqlite';

interface Row {
  id: number;
  title: string;
  score: number | null;
  ratio: number;
}

class Store {
  private db: DatabaseSync;

  constructor(path: string) {
    this.db = new DatabaseSync(path);
    this.db.exec(`
      CREATE TABLE items (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL UNIQUE,
        score INTEGER,
        ratio REAL NOT NULL
      )
    `);
  }

  add(title: string, score: number | null, ratio: number): number {
    const statement: StatementSync = this.db.prepare('INSERT INTO items (title, score, ratio) VALUES (?, ?, ?)');
    const result = statement.run(title, score, ratio);
    return Number(result.lastInsertRowid);
  }

  byTitle(title: string): Row | undefined {
    return this.db.prepare('SELECT id, title, score, ratio FROM items WHERE title = :title').get({ title }) as Row | undefined;
  }

  all(): Row[] {
    return this.db.prepare('SELECT id, title, score, ratio FROM items ORDER BY id').all() as Row[];
  }

  bump(): number {
    const result = this.db.prepare('UPDATE items SET ratio = ratio * 2 WHERE score IS NOT NULL').run();
    return Number(result.changes);
  }

  columns(): string[] {
    return Object.keys(this.db.prepare('SELECT * FROM items WHERE id = 1').get() as Record<string, unknown>);
  }

  widen(): void {
    this.db.exec('ALTER TABLE items ADD COLUMN tag TEXT');
  }

  close(): void {
    this.db.close();
  }
}

const store = new Store(':memory:');
console.log(store.add('first', 3, 0.5));
console.log(store.add('second', null, 1.25));
console.log(store.add('third', -7, 2));
try {
  store.add('first', 1, 1);
} catch (error) {
  console.log('caught', (error as Error).message);
}
console.log(store.bump());
for (const row of store.all()) {
  console.log(row.id, row.title, row.score === null ? 'null' : row.score, row.ratio);
}
const second = store.byTitle('second');
console.log(second === undefined ? 'missing' : second.title);
console.log(store.byTitle('nope') === undefined);
console.log(store.columns().join(','));
store.widen();
console.log(store.columns().join(','));
store.close();
try {
  store.all();
} catch (error) {
  console.log('closed', (error as Error).message);
}

const scratch = new DatabaseSync(':memory:');
const blobRow = scratch.prepare("SELECT X'00ff10' AS b").get() as { b: Uint8Array };
console.log(Object.prototype.toString.call(blobRow.b), blobRow.b.length, blobRow.b[1]);
scratch.close();
