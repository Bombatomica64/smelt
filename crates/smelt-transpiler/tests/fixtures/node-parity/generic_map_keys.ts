class LRUCache<K, V> {
  #map = new Map<K, V>();
  constructor(private readonly capacity: number) {}
  get(key: K): V | undefined {
    if (!this.#map.has(key)) return undefined;
    const v = this.#map.get(key)!;
    this.#map.delete(key); this.#map.set(key, v);
    return v;
  }
  put(key: K, value: V): void {
    if (this.#map.has(key)) this.#map.delete(key);
    else if (this.#map.size >= this.capacity) {
      const oldest = this.#map.keys().next().value as K;
      this.#map.delete(oldest);
    }
    this.#map.set(key, value);
  }
  keys(): K[] { return [...this.#map.keys()]; }
}
const lru = new LRUCache<string, number>(2);
lru.put("a", 1); lru.put("b", 2);
console.log(lru.get("a"));
lru.put("c", 3);
console.log(lru.get("b"), lru.keys());
lru.put("a", 10);
console.log(lru.keys(), lru.get("a"));
const lru2 = new LRUCache<number, string[]>(1);
lru2.put(1, ["x"]); lru2.put(2, ["y", "z"]);
console.log(lru2.get(1), lru2.get(2));
