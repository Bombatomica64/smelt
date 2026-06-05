function read(): unknown {
  const SymbolKey = Symbol("kind");
  const item = { [SymbolKey]: "cat", [Symbol("inline")]: "dog", 2: 123 };
  return item[SymbolKey];
}
