// A lifted generic caller hands its own `T[]` to a lifted generic callee that
// mutates it in place. The call binds the callee's `T` to the caller's `T`, so
// the list is borrowed straight through (`&mut heap`): both functions keep
// their type parameter, and the callee's writes reach the caller's list.
type Compare<T> = (left: T, right: T) => number;

function siftDown<T>(heap: T[], index: number, compare: Compare<T>): void {
  const left = index * 2 + 1;
  if (left < heap.length && compare(heap[left], heap[index]) < 0) {
    const held = heap[left];
    heap[left] = heap[index];
    heap[index] = held;
  }
}

function heapify<T>(heap: T[], compare: Compare<T>): void {
  for (let index = Math.floor(heap.length / 2) - 1; index >= 0; index--) {
    siftDown(heap, index, compare);
  }
}

function smallestFirst<T>(data: readonly T[], compare: Compare<T>, size: number): T[] {
  const heap = data.slice(0, size);
  heapify(heap, compare);
  return heap;
}

console.log(smallestFirst([5, 3, 1, 4], (left, right) => left - right, 3).join(","));
console.log(smallestFirst(["b", "a"], (left, right) => left.localeCompare(right), 2).join(","));
