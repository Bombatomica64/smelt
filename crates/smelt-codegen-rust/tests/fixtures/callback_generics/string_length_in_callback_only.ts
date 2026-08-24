// Fixture: string_length_in_callback_only
// Area: site_pinning
// Guards: a non-generic `.map` whose callback reads `.length` off the element.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function lens(ss: string[]): boolean[] {
  return ss.map((v: string) => v.length > 1);
}
