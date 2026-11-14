// A second entry: it runs after `app`, and `shared` does not run again.
import { shared } from "./shared.ts";
console.log("second body", shared);
