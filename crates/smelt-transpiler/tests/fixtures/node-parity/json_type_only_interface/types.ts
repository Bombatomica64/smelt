// Imported by `main.ts` only for its types, so it lowers after the module
// that serializes its interfaces.
export interface Duration {
  years?: number;
  months?: number;
  days?: number;
  label?: string;
}

export interface Span {
  from: Duration;
  parts: Duration[];
}
