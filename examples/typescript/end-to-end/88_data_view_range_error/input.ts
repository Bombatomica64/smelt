// A fallible stdlib MEMBER whose throw reaches a source `catch`.
//
// `DataView.prototype.getX/setX` throws a `RangeError` when the window it
// addresses is not wholly inside the view, and JavaScript makes that error
// catchable. Every earlier fallible builtin — `JSON.parse`, the URI decoders,
// the base64 pair — was a free FUNCTION; this is the first member, and it is
// what forced the general shape: MIR's exception edges live on TERMINATORS
// (`Terminator::Call` carries `unwind`, a `Statement::Assign` carries nothing),
// so an accessor lowered as an rvalue could never reach a handler — the catch
// block would have no predecessor at all and the throw would become the
// infallible helper's silent zero. The accessor is therefore a call to
// `BuiltinFn::DataViewAccess`, exactly as `JSON.parse` is, and the width and
// the direction ride on the builtin rather than being re-read from the member
// name. See `blocker-logs/standards-throwing-rvalue.md`.
//
// The view is a PARAMETER everywhere below rather than a module-level `const`,
// because a module-level binding read from inside a function arrives through
// the erased globals record and the accessor rule needs the receiver's type.
//
// Every handler reads the brand through an `instanceof` NARROWING rather than
// an `as` cast, for the reason `76_base64_globals` gives: TypeScript types a
// `catch` binding `unknown` by its own rule, so the cast form stores the caught
// value in a second erased local for no gain, where the guard is the
// runtime-narrowing boundary the value actually crosses.
//
// Every line below is diffed against Node.

const buffer = new ArrayBuffer(4);
const view = new DataView(buffer);

// In range, so no throw: the fallible spelling did not make the ordinary read
// fallible for the program.
view.setInt16(2, 258);
console.log(view.getInt16(2), view.getUint8(2), view.getUint8(3));

// The last two bytes are readable at offset 2 and not at offset 3, and the
// message the handler reads is Node's own.
console.log(readAt(view, 2));
console.log(readAt(view, 3));

function readAt(target: DataView, offset: number): string {
  try {
    return `ok:${target.getInt16(offset)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return `E:${error.name}:${error.message}`;
    }
    return "E:unexpected";
  }
}

// A WRITE is fallible in the same way and on the same channel.
console.log(writeAt(view, 0));
console.log(writeAt(view, 4));
console.log(view.getInt32(0));

function writeAt(target: DataView, offset: number): string {
  try {
    target.setInt32(offset, 7);
    return "wrote";
  } catch (error) {
    if (error instanceof RangeError) {
      return `E:${error.name}`;
    }
    return "E:unexpected";
  }
}

// The width comes from the ACCESSOR, not from the view, so the same offset on
// the same view is in range for one call and out of range for another.
console.log(readWide(view, 2), readWide(view, 4), readWide(view, 8));

function readWide(target: DataView, width: number): string {
  try {
    if (width === 2) {
      return `ok:${target.getUint16(0)}`;
    }
    if (width === 4) {
      return `ok:${target.getUint32(0)}`;
    }
    return `ok:${target.getFloat64(0)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return `E:${error.name}`;
    }
    return "E:unexpected";
  }
}

// `ToIndex` is not a cast, and the accessor runs it before the window test: a
// fractional offset TRUNCATES, and a NEGATIVE or non-representable one is the
// same `RangeError` an out-of-window offset gets. Clamping it to zero instead
// would have answered byte 0 for a negative read.
console.log(readAt(view, 1.9), readAt(view, 2));
console.log(readAt(view, -1), readAt(view, Infinity));

// A WINDOW is an offset and a length into the same storage, so the bound is the
// window's, not the buffer's: byte 1 of the window is inside it, and byte 2 is
// not, even though the buffer has the bytes.
const windowed = new DataView(buffer, 2, 2);
console.log(readByte(windowed, 1), readByte(windowed, 2));

function readByte(target: DataView, offset: number): string {
  try {
    return `ok:${target.getUint8(offset)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return `E:${error.name}`;
    }
    return "E:unexpected";
  }
}

// The brand is a real `RangeError`, so it is an `Error` too.
console.log(brandOf(windowed));

function brandOf(target: DataView): string {
  try {
    target.getFloat64(0);
    return "no throw";
  } catch (error) {
    return `${error instanceof RangeError} ${error instanceof Error}`;
  }
}

// And the throw PROPAGATES out of a function that does not catch it, which is
// the part a `Statement::Assign` could not express at all: the callee's unwind
// edge has to leave the callee.
console.log(propagated(windowed));

function uncaught(target: DataView): number {
  return target.getInt32(0);
}

function propagated(target: DataView): string {
  try {
    return `ok:${uncaught(target)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return `E:${error.name}`;
    }
    return "E:unexpected";
  }
}
