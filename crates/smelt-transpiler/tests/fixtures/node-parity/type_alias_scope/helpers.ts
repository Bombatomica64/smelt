export type Iterator<n extends number, it extends any[] = []> = it["length"] extends n ? it : Iterator<n, [any, ...it]>;
export const three: Iterator<3>["length"] = 3;
