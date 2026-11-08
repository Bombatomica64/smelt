// An erased object projected into an interface reads each property by the
// name the source spells, including names a Rust identifier cannot round-trip.

interface Flags {
  isHTML: boolean;
  URL: string;
  snake_case: number;
  camelCase: number;
}

function describe(flags: Flags): string {
  return `${flags.isHTML} ${flags.URL} ${flags.snake_case} ${flags.camelCase}`;
}

const raw: unknown = JSON.parse('{"isHTML": true, "URL": "u", "snake_case": 3, "camelCase": 4, "is_html": false, "url": "wrong", "snakeCase": 9}');
console.log(describe(raw as Flags));
