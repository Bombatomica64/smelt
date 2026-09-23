// A module-level async arrow awaited inside a loop body whose earlier
// iterations can `continue`: the awaited call's result must be declared on
// every path, and the arrow's own body (try/catch included) must run.
const verify = async (signature: string, value: string, secret: string): Promise<boolean> => {
  try {
    if (signature.length === 0) {
      throw new Error("empty");
    }
    return signature === value + secret;
  } catch {
    return false;
  }
};

const parseSigned = async (entries: string[], secret: string): Promise<Record<string, string | false>> => {
  const parsed: Record<string, string | false> = {};
  for (const entry of entries) {
    const dot = entry.lastIndexOf(".");
    if (dot < 0) {
      continue;
    }
    const value = entry.substring(0, dot);
    const signature = entry.substring(dot + 1);
    if (signature.length > 10) {
      continue;
    }
    const verified = await verify(signature, value, secret);
    parsed[value] = verified ? value : false;
  }
  return parsed;
};

const result = await parseSigned(["a.ak", "b.bad", "nodot", "c.", "d.dk", "e.waytoolongsignature"], "k");
for (const key of Object.keys(result)) {
  console.log(key + " -> " + result[key]);
}
