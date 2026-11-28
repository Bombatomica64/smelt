// `Class.staticMethod` read as a value is the associated function itself.
class Version {
  constructor(readonly major: number, readonly minor: number) {}
  static parse(text: string): Version {
    const [major, minor] = text.split(".").map(Number);
    return new Version(major, minor ?? 0);
  }
  static compare(a: Version, b: Version): number {
    return a.major - b.major || a.minor - b.minor;
  }
  static describe(v: Version): string { return `v${v.major}.${v.minor}`; }
}
const versions = ["1.10", "1.2", "0.9"].map(Version.parse);
console.log(versions.map(Version.describe).join(" "));
console.log([...versions].sort(Version.compare).map(Version.describe).join(" < "));
const parse: (text: string) => Version = Version.parse;
console.log(parse("3.4").minor);
const table = new Map<string, (v: Version) => string>([["describe", Version.describe]]);
console.log(table.get("describe")?.(new Version(7, 1)));
