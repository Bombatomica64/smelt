"use strict";

/**
 * Sandboxed Node guest for materializing standard TypeScript decorators.
 *
 * Input is one JSON request path. Output is exactly one specialization
 * manifest written through the original stdout handle.
 */

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");

const decoratorRecords = [];

/** Begin one standard __esDecorate invocation. */
globalThis.__smeltRecordDecorator = function recordDecorator(
  filename,
  ctor,
  context,
  initializers,
  extraInitializers,
) {
  decoratorRecords.push({
    filename,
    ctor,
    context,
    initializers,
    extraInitializers,
    descriptor: null,
  });
};

/** Finish one standard __esDecorate invocation with its final descriptor. */
globalThis.__smeltFinishDecorator = function finishDecorator(
  filename,
  context,
  descriptor,
) {
  const record = [...decoratorRecords]
    .reverse()
    .find(
      (candidate) =>
        candidate.filename === filename &&
        candidate.context === context &&
        candidate.descriptor === null,
    );
  if (record) record.descriptor = descriptor;
};

/** Fail one unsupported or nondeterministic definition-time operation. */
function rejectOperation(name) {
  throw new Error(
    `native specialization adapter required: forbidden definition-time operation '${name}'`,
  );
}

/** Install deterministic runtime guards before loading project artifacts. */
function installGuards(policy) {
  Date.now = () => rejectOperation("Date.now");
  Math.random = () => rejectOperation("Math.random");
  const deniedModules = new Set([
    "child_process",
    "node:child_process",
    "cluster",
    "node:cluster",
    "dgram",
    "node:dgram",
    "http",
    "node:http",
    "https",
    "node:https",
    "net",
    "node:net",
    "worker_threads",
    "node:worker_threads",
  ]);
  const originalLoad = Module._load;
  Module._load = function guardedLoad(request, parent, isMain) {
    if (deniedModules.has(request)) {
      rejectOperation(request);
    }
    return originalLoad.call(this, request, parent, isMain);
  };
  for (const name of [
    "appendFile",
    "appendFileSync",
    "chmod",
    "chmodSync",
    "copyFile",
    "copyFileSync",
    "createWriteStream",
    "mkdir",
    "mkdirSync",
    "rename",
    "renameSync",
    "rm",
    "rmSync",
    "unlink",
    "unlinkSync",
    "writeFile",
    "writeFileSync",
  ]) {
    fs[name] = () => rejectOperation(`filesystem:${name}`);
  }
  const allowed = Object.freeze({ ...policy.environment });
  process.env = new Proxy(allowed, {
    get(target, property) {
      if (typeof property === "symbol") {
        return target[property];
      }
      if (!Object.prototype.hasOwnProperty.call(target, property)) {
        rejectOperation(`environment:${property}`);
      }
      return target[property];
    },
    set() {
      rejectOperation("environment:write");
    },
  });
}

/** Stable manifest static type for one materialized JavaScript value. */
function staticType(value) {
  if (value === null || value === undefined) return { kind: "null" };
  if (typeof value === "boolean") return { kind: "bool" };
  if (typeof value === "bigint") return { kind: "int" };
  if (typeof value === "number") {
    return Number.isSafeInteger(value) ? { kind: "int" } : { kind: "float" };
  }
  if (typeof value === "string") return { kind: "string" };
  if (Array.isArray(value)) {
    const item = value.length === 0 ? namedType("unknown") : commonType(value);
    return { kind: "list", value: item };
  }
  if (value instanceof Set) {
    return { kind: "set", value: commonType([...value]) };
  }
  if (value instanceof Map) {
    return {
      kind: "dict",
      value: [commonType([...value.keys()]), commonType([...value.values()])],
    };
  }
  if (typeof value === "function") {
    return { kind: "function", value: functionSignature(value) };
  }
  return namedType(qualifiedTypeName(value));
}

/** Named static type helper. */
function namedType(name) {
  return { kind: "named", value: name };
}

/** Common static type for a materialized collection. */
function commonType(values) {
  if (values.length === 0) return namedType("unknown");
  const first = staticType(values[0]);
  const encoded = JSON.stringify(first);
  return values.every((value) => JSON.stringify(staticType(value)) === encoded)
    ? first
    : { kind: "dynamic_metadata" };
}

/** Stable concrete class name for a JavaScript object. */
function qualifiedTypeName(value) {
  if (typeof value === "function") return value.name || "anonymous";
  const constructor = value && value.constructor;
  return constructor && constructor.name ? constructor.name : "Object";
}

/** Source-independent callable signature available after JavaScript emission. */
function functionSignature(callable) {
  return {
    parameters: Array.from({ length: callable.length }, (_, index) => ({
      name: `arg${index}`,
      ty: { kind: "dynamic_metadata" },
      kind: "positional",
      default: null,
      annotation: null,
    })),
    return_type: { kind: "dynamic_metadata" },
    is_async: callable.constructor && callable.constructor.name === "AsyncFunction",
    throws: true,
  };
}

/** Reference-preserving value graph serializer. */
class Serializer {
  constructor(modules) {
    this.modules = modules;
    this.nodes = [];
    this.identities = new Map();
    this.adapters = new Map();
  }

  /** Record one stable native adapter requirement. */
  addAdapter(id, reason, span = null) {
    if (!this.adapters.has(id)) {
      this.adapters.set(id, { id, version: "1", reason, span });
    }
  }

  /** Serialize one value and return its stable graph identity. */
  value(value, moduleRecord = null, qualifiedName = null) {
    const identityBearing =
      (typeof value === "object" && value !== null) || typeof value === "function";
    if (identityBearing && this.identities.has(value)) {
      return this.identities.get(value);
    }
    const id = this.nodes.length;
    this.nodes.push({});
    if (identityBearing) this.identities.set(value, id);
    const [ty, payload] = this.payload(value, moduleRecord, qualifiedName);
    this.nodes[id] = { id, ty, value: payload };
    return id;
  }

  /** Build one graph node payload. */
  payload(value, moduleRecord, qualifiedName) {
    if (value === null || value === undefined) {
      return [{ kind: "null" }, { kind: "null" }];
    }
    if (typeof value === "boolean") {
      return [{ kind: "bool" }, { kind: "bool", value }];
    }
    if (typeof value === "bigint") {
      return [{ kind: "int" }, { kind: "int", value: value.toString() }];
    }
    if (typeof value === "number") {
      if (Number.isSafeInteger(value)) {
        return [{ kind: "int" }, { kind: "int", value: value.toString() }];
      }
      if (!Number.isFinite(value)) {
        this.addAdapter(
          "typescript.non-finite-number",
          "non-finite numbers require a concrete specialization adapter",
        );
        return [{ kind: "float" }, { kind: "float", value: 0 }];
      }
      return [{ kind: "float" }, { kind: "float", value }];
    }
    if (typeof value === "string") {
      return [{ kind: "string" }, { kind: "string", value }];
    }
    if (Array.isArray(value)) {
      const values = value.map((item) => this.value(item, moduleRecord));
      return [staticType(value), { kind: "list", value: values }];
    }
    if (value instanceof Set) {
      const entries = [...value].sort(stableCompare);
      return [
        staticType(value),
        {
          kind: "set",
          value: entries.map((item) => this.value(item, moduleRecord)),
        },
      ];
    }
    if (value instanceof Map) {
      const entries = [...value.entries()].sort(([left], [right]) =>
        stableCompare(left, right),
      );
      return [
        staticType(value),
        {
          kind: "dict",
          value: entries.map(([key, item]) => [
            this.value(key, moduleRecord),
            this.value(item, moduleRecord),
          ]),
        },
      ];
    }
    if (typeof value === "function") {
      if (isClass(value)) {
        return [
          namedType(value.name || qualifiedName || "anonymous"),
          {
            kind: "class_ref",
            value: {
              module: moduleRecord ? moduleRecord.name : "",
              qualified_name: qualifiedName || value.name || "anonymous",
            },
          },
        ];
      }
      const provenance = callableProvenance(
        value,
        moduleRecord,
        qualifiedName || value.name || "anonymous",
        "free",
        this,
      );
      return [
        { kind: "function", value: functionSignature(value) },
        { kind: "function_ref", value: provenance },
      ];
    }
    const fields = Object.fromEntries(
      Reflect.ownKeys(value)
        .filter((key) => typeof key === "string")
        .sort()
        .map((key) => [key, this.value(value[key], moduleRecord)]),
    );
    return [
      namedType(qualifiedTypeName(value)),
      {
        kind: "instance",
        value: { class: qualifiedTypeName(value), fields },
      },
    ];
  }
}

/** Deterministic comparison for set and map graph ordering. */
function stableCompare(left, right) {
  return String(left).localeCompare(String(right));
}

/** Whether a runtime function is class-like. */
function isClass(value) {
  if (typeof value !== "function") return false;
  const source = Function.prototype.toString.call(value);
  return source.startsWith("class ");
}

/** Locate callable JavaScript bytes in its canonical emitted module. */
function callableSource(callable, moduleRecord) {
  if (!moduleRecord) return null;
  const source = Function.prototype.toString.call(callable);
  if (source.includes("[native code]")) return null;
  const start = moduleRecord.emittedText.indexOf(source);
  if (start < 0) return null;
  return {
    hash: crypto.createHash("sha256").update(source).digest("hex"),
  };
}

/** Approximate one emitted callable's declaration span in canonical TypeScript. */
function originalSourceSpan(moduleRecord, qualifiedName) {
  if (!moduleRecord) return null;
  const name = qualifiedName
    .replace(/\.(get|set)$/, "")
    .split(".")
    .at(-1);
  if (!name) return null;
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const patterns = [
    new RegExp(`\\bclass\\s+${escaped}\\b`),
    new RegExp(`\\bfunction\\s+${escaped}\\b`),
    new RegExp(`\\b(?:get|set|accessor)\\s+${escaped}\\b`),
    new RegExp(`(?:^|\\n)\\s*(?:static\\s+)?${escaped}\\s*(?:[=:;])`, "u"),
    new RegExp(`\\b${escaped}\\s*\\(`),
  ];
  let match = null;
  for (const pattern of patterns) {
    const candidate = pattern.exec(moduleRecord.sourceText);
    if (candidate && (!match || candidate.index < match.index)) {
      match = candidate;
    }
  }
  if (!match) return null;
  const lineEnd = moduleRecord.sourceText.indexOf("\n", match.index);
  const endIndex = lineEnd < 0 ? moduleRecord.sourceText.length : lineEnd;
  return {
    file: moduleRecord.source_path,
    start: Buffer.byteLength(moduleRecord.sourceText.slice(0, match.index)),
    end: Buffer.byteLength(moduleRecord.sourceText.slice(0, endIndex)),
  };
}

/** Build callable provenance or request a precise native adapter. */
function callableProvenance(
  callable,
  moduleRecord,
  qualifiedName,
  bindingMode,
  serializer,
) {
  const resolved = callableSource(callable, moduleRecord);
  const sourceSpan = originalSourceSpan(moduleRecord, qualifiedName);
  if (!resolved || !sourceSpan) {
    const id = `typescript.native-callable:${qualifiedName}`;
    serializer.addAdapter(
      id,
      `callable '${qualifiedName}' cannot be mapped to emitted TypeScript source`,
    );
  }
  return {
    language: "type_script",
    module: moduleRecord ? moduleRecord.name : "",
    qualified_name: qualifiedName,
    span:
      sourceSpan ||
      { file: moduleRecord ? moduleRecord.source_path : "", start: 0, end: 0 },
    code_hash: resolved ? resolved.hash : "",
    captures: {},
    binding_mode: bindingMode,
  };
}

/** Materialize one final callable binding. */
function functionDefinition(
  name,
  callable,
  moduleRecord,
  bindingMode,
  serializer,
  qualifiedName = name,
) {
  return {
    name,
    signature: functionSignature(callable),
    callable: callableProvenance(
      callable,
      moduleRecord,
      qualifiedName,
      bindingMode,
      serializer,
    ),
    wrapper_chain: [],
    binding_mode: bindingMode,
  };
}

/** Return the metadata object installed through Symbol.metadata. */
function decoratorMetadata(classValue) {
  for (const symbol of Object.getOwnPropertySymbols(classValue)) {
    if (
      symbol === Symbol.metadata ||
      symbol.description === "Symbol.metadata" ||
      symbol.description === "metadata"
    ) {
      return classValue[symbol];
    }
  }
  return undefined;
}

/** Instrument canonical TypeScript's standard decorator helper in memory. */
function instrumentDecorators(source) {
  if (!source.includes("var __esDecorate")) return source;
  const opening =
    "function (ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) {";
  const instrumentedOpening = `${opening}
    globalThis.__smeltRecordDecorator(__filename, ctor, contextIn, initializers, extraInitializers);`;
  const closing = "    done = true;";
  const instrumentedClosing =
    "    globalThis.__smeltFinishDecorator(__filename, contextIn, descriptor);\n" +
    closing;
  return source
    .replace(opening, instrumentedOpening)
    .replace(closing, instrumentedClosing);
}

/** Decorator records associated with one final class export. */
function classDecoratorRecords(classValue, moduleRecord) {
  const relevant = decoratorRecords.filter(
    (record) => path.resolve(record.filename) === path.resolve(moduleRecord.emitted_path),
  );
  const metadata = decoratorMetadata(classValue);
  const classIndex = relevant.findIndex(
    (record) =>
      record.context.kind === "class" &&
      (record.context.name === classValue.name ||
        (metadata && record.context.metadata === metadata)),
  );
  if (classIndex < 0) {
    return relevant.filter(
      (record) =>
        record.ctor === classValue ||
        (metadata && record.context.metadata === metadata),
    );
  }
  let start = classIndex;
  while (start > 0 && relevant[start - 1].context.kind !== "class") {
    start -= 1;
  }
  return relevant.slice(start, classIndex + 1);
}

/** Materialize a decorated class after static initialization. */
function classDefinition(classValue, moduleRecord, serializer) {
  const methods = [];
  const descriptors = [];
  const fields = [];
  const initializers = [];
  const prototypeDescriptors = Object.getOwnPropertyDescriptors(
    classValue.prototype || {},
  );
  for (const name of Object.keys(prototypeDescriptors).sort()) {
    if (name === "constructor") continue;
    const descriptor = prototypeDescriptors[name];
    if (typeof descriptor.value === "function") {
      methods.push(
        functionDefinition(
          name,
          descriptor.value,
          moduleRecord,
          "instance",
          serializer,
          `${classValue.name}.${name}`,
        ),
      );
    } else if (descriptor.get || descriptor.set) {
      const descriptorValue = serializer.value(
        {
          configurable: descriptor.configurable,
          enumerable: descriptor.enumerable,
        },
        moduleRecord,
      );
      descriptors.push({
        name,
        value: descriptorValue,
        read_type: { kind: "dynamic_metadata" },
        write_type: descriptor.set ? { kind: "dynamic_metadata" } : null,
        getter: descriptor.get
          ? callableProvenance(
              descriptor.get,
              moduleRecord,
              `${classValue.name}.${name}.get`,
              "instance",
              serializer,
            )
          : null,
        setter: descriptor.set
          ? callableProvenance(
              descriptor.set,
              moduleRecord,
              `${classValue.name}.${name}.set`,
              "instance",
              serializer,
            )
          : null,
        data_descriptor: Boolean(descriptor.set),
      });
    }
  }

  const staticValues = {};
  for (const name of Object.getOwnPropertyNames(classValue).sort()) {
    if (["length", "name", "prototype"].includes(name)) continue;
    const value = classValue[name];
    if (typeof value === "function") {
      methods.push(
        functionDefinition(
          name,
          value,
          moduleRecord,
          "class",
          serializer,
          `${classValue.name}.${name}`,
        ),
      );
    } else {
      staticValues[name] = serializer.value(value, moduleRecord);
    }
  }
  const metadata = [];
  const materializedMetadata = decoratorMetadata(classValue);
  if (materializedMetadata !== undefined) {
    metadata.push({
      key: "typescript.decorators",
      value: serializer.value(materializedMetadata, moduleRecord),
    });
  }
  const mro = [];
  let current = classValue;
  while (
    typeof current === "function" &&
    current !== Function.prototype &&
    current !== Object
  ) {
    mro.push(current.name || "anonymous");
    current = Object.getPrototypeOf(current);
  }
  const base = Object.getPrototypeOf(classValue);
  const bases =
    typeof base === "function" && base !== Function.prototype
      ? [serializer.value(base, moduleRecord, base.name)]
      : [];
  const recordedTargets = classDecoratorRecords(classValue, moduleRecord);
  let initializerOrder = 0;
  const recordedInitializers = new Set();
  for (const record of recordedTargets) {
    const context = record.context;
    const memberName =
      typeof context.name === "symbol"
        ? context.name.description || context.name.toString()
        : String(context.name);
    if (context.kind === "field" || context.kind === "accessor") {
      fields.push({
        name: memberName,
        ty: { kind: "dynamic_metadata" },
        default: null,
        is_static: Boolean(context.static),
        is_private: Boolean(context.private),
        span: originalSourceSpan(moduleRecord, memberName),
      });
    }
    if (
      context.kind === "accessor" &&
      !descriptors.some((descriptor) => descriptor.name === memberName)
    ) {
      const descriptorValue = serializer.value(
        {
          kind: context.kind,
          private: Boolean(context.private),
          static: Boolean(context.static),
        },
        moduleRecord,
      );
      descriptors.push({
        name: memberName,
        value: descriptorValue,
        read_type: { kind: "dynamic_metadata" },
        write_type: { kind: "dynamic_metadata" },
        getter:
          record.descriptor && typeof record.descriptor.get === "function"
            ? callableProvenance(
              record.descriptor.get,
              moduleRecord,
              `${classValue.name}.${memberName}.get`,
              context.static ? "class" : "instance",
              serializer,
            )
            : null,
        setter:
          record.descriptor && typeof record.descriptor.set === "function"
            ? callableProvenance(
              record.descriptor.set,
              moduleRecord,
              `${classValue.name}.${memberName}.set`,
              context.static ? "class" : "instance",
              serializer,
            )
            : null,
        data_descriptor: true,
      });
    }
    for (const initializer of [
      ...(record.initializers || []),
      ...(record.extraInitializers || []),
    ]) {
      if (recordedInitializers.has(initializer)) continue;
      recordedInitializers.add(initializer);
      initializers.push({
        order: initializerOrder,
        kind: context.static || context.kind === "class" ? "static" : "instance",
        callable: callableProvenance(
          initializer,
          moduleRecord,
          `${classValue.name}.${memberName}`,
          context.static || context.kind === "class" ? "class" : "instance",
          serializer,
        ),
      });
      initializerOrder += 1;
    }
  }
  return {
    mro,
    bases,
    fields,
    descriptors,
    methods,
    static_values: staticValues,
    slots: [],
    metadata,
    constructor: {
      signature: functionSignature(classValue),
      replacement: callableProvenance(
        classValue,
        moduleRecord,
        classValue.name || "anonymous",
        "class",
        serializer,
      ),
    },
    initializers,
  };
}

/** Reject TypeScript's legacy decorator and parameter-decorator transforms. */
function validateDecoratorAbi(moduleRecord) {
  if (/\b__param\s*\(/u.test(moduleRecord.emittedText)) {
    throw new Error(
      "smelt::typescript-parameter-decorators: parameter decorators are not part of the standard decorator ABI",
    );
  }
  if (/\b__decorate\s*\(/u.test(moduleRecord.emittedText)) {
    throw new Error(
      "smelt::typescript-legacy-decorators: experimentalDecorators output is not supported",
    );
  }
}

/** Materialize one exported binding. */
function materializedDefinition(name, value, moduleRecord, serializer) {
  const definitionSpan = originalSourceSpan(moduleRecord, name);
  const source = {
    module: moduleRecord.name,
    qualified_name: name,
    span:
      definitionSpan ||
      { file: moduleRecord.source_path, start: 0, end: 0 },
  };
  if (isClass(value)) {
    return {
      original_name: name,
      binding_name: name,
      source,
      binding_type: namedType(value.name || name),
      definition: {
        kind: "class",
        ...classDefinition(value, moduleRecord, serializer),
      },
    };
  }
  if (typeof value === "function") {
    const definition = functionDefinition(
      name,
      value,
      moduleRecord,
      "free",
      serializer,
    );
    return {
      original_name: name,
      binding_name: name,
      source,
      binding_type: { kind: "function", value: definition.signature },
      definition: { kind: "function", ...definition },
    };
  }
  const valueId = serializer.value(value, moduleRecord, name);
  return {
    original_name: name,
    binding_name: name,
    source,
    binding_type: serializer.nodes[valueId].ty,
    definition: { kind: "value", value: valueId },
  };
}

/** Load every CommonJS artifact once in graph order while capturing output. */
function loadModules(request, events) {
  const loaded = [];
  const originalStdout = process.stdout.write.bind(process.stdout);
  const originalStderr = process.stderr.write.bind(process.stderr);
  process.stdout.write = (chunk, encoding, callback) => {
    events.push({ kind: "output", stderr: false, text: String(chunk) });
    if (typeof callback === "function") callback();
    return true;
  };
  process.stderr.write = (chunk, encoding, callback) => {
    events.push({ kind: "output", stderr: true, text: String(chunk) });
    if (typeof callback === "function") callback();
    return true;
  };
  const candidatePaths = new Set(
    request.modules.map((record) => path.resolve(record.emitted_path)),
  );
  const originalJavaScriptLoader = Module._extensions[".js"];
  Module._extensions[".js"] = function loadInstrumented(module, filename) {
    if (!candidatePaths.has(path.resolve(filename))) {
      return originalJavaScriptLoader(module, filename);
    }
    const source = fs.readFileSync(filename, "utf8");
    module._compile(instrumentDecorators(source), filename);
  };
  try {
    for (const record of request.modules) {
      const loadedRecord = {
        ...record,
        sourceText: fs.readFileSync(record.source_path, "utf8"),
        emittedText: fs.readFileSync(record.emitted_path, "utf8"),
      };
      validateDecoratorAbi(loadedRecord);
      loadedRecord.exports = require(record.emitted_path);
      loaded.push(loadedRecord);
    }
  } finally {
    Module._extensions[".js"] = originalJavaScriptLoader;
    process.stdout.write = originalStdout;
    process.stderr.write = originalStderr;
  }
  return { loaded, originalStdout };
}

/** Build the versioned specialization manifest. */
function buildManifest(request) {
  installGuards(request.sandbox_policy);
  const events = [];
  const { loaded, originalStdout } = loadModules(request, events);
  const serializer = new Serializer(loaded);
  const modules = loaded.map((record) => {
    const definitions = [];
    const globals = {};
    for (const name of Object.keys(record.exports).sort()) {
      if (name.startsWith("__")) continue;
      const value = record.exports[name];
      definitions.push(materializedDefinition(name, value, record, serializer));
      globals[name] = serializer.value(value, record, name);
    }
    return { path: record.source_path, definitions, globals };
  });
  return {
    manifest: {
      smelt_version: request.smelt_version,
      schema_version: request.schema_version,
      language: "type_script",
      host_runtime_version: `${process.version}; TypeScript ${request.typescript_version}; decorators ${request.decorators_revision}`,
      hashes: request.hashes,
      sandbox_policy: request.sandbox_policy,
      modules,
      values: { nodes: serializer.nodes },
      effects: events,
      required_adapters: [...serializer.adapters.values()],
    },
    originalStdout,
  };
}

/** Read one request and emit one compact JSON manifest. */
function main() {
  if (process.argv.length !== 3) {
    throw new Error("expected one Node specialization request path");
  }
  const request = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
  const { manifest, originalStdout } = buildManifest(request);
  originalStdout(JSON.stringify(manifest));
}

main();
