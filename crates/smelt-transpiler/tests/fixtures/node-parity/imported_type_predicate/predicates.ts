export function isText(value?: unknown): value is string {
  return typeof value === "string";
}

/** True for a plain key; false for a path it would still accept, like `a.b`. */
export function isPlainKey(value: string | string[]): value is string {
  return typeof value === "string" && !value.includes(".");
}

export function assertText(value: unknown): asserts value is string {
  if (typeof value !== "string") {
    throw new Error("not text");
  }
}
