// Minimal JSON Schema (2020-12 subset) validator for the TS consumer side of
// the lingxi.wire protocol round-trip gate (R01-A03).
//
// This validates decoded JSON against the *generated* schemas in
// contracts/generated/jsonschema/. It implements exactly the schema subset
// the generator (schemars 1.x over the lingxi-protocol types) emits:
//   type, properties, required, additionalProperties (bool or schema),
//   items, anyOf/oneOf/allOf, enum, const, pattern, $ref (local #/$defs/...),
//   $defs, description/title (ignored).
// Anything outside that subset is a hard validator error, never a silent
// pass — the same "no silent degradation" rule as the Rust side.

export class SchemaValidationError extends Error {
  /**
   * @param {string[]} errors
   */
  constructor(errors) {
    super(`schema validation failed:\n${errors.map((e) => `  - ${e}`).join("\n")}`);
    this.errors = errors;
  }
}

/**
 * Validate `value` against the root schema `schema` (a parsed generated
 * schema document, $defs included). Throws SchemaValidationError listing
 * every violation found.
 *
 * @param {unknown} value
 * @param {Record<string, unknown>} schema
 * @param {string} [path]
 */
export function validateAgainstSchema(value, schema, path = "$") {
  const errors = [];
  validate(value, schema, schema, path, errors);
  if (errors.length > 0) throw new SchemaValidationError(errors);
}

/**
 * @param {unknown} value
 * @param {unknown} schema
 * @param {Record<string, unknown>} root
 * @param {string} path
 * @param {string[]} errors
 */
function validate(value, schema, root, path, errors) {
  if (schema === true || schema === undefined || schema === null) return;
  if (schema === false) {
    errors.push(`${path}: schema is false`);
    return;
  }
  if (typeof schema !== "object" || Array.isArray(schema)) {
    errors.push(`${path}: malformed schema node`);
    return;
  }
  const s = /** @type {Record<string, unknown>} */ (schema);

  if (typeof s.$ref === "string") {
    const target = resolveRef(s.$ref, root);
    if (target === undefined) {
      errors.push(`${path}: unresolvable $ref ${s.$ref}`);
      return;
    }
    validate(value, target, root, path, errors);
    return;
  }

  if (Array.isArray(s.anyOf)) {
    const hits = s.anyOf.filter((sub) => subErrors(value, sub, root, path).length === 0);
    if (hits.length === 0) {
      const detail = s.anyOf
        .map((sub, i) => `branch ${i}: ${subErrors(value, sub, root, path).slice(0, 3).join("; ")}`)
        .join(" | ");
      errors.push(`${path}: matches no anyOf branch (${detail})`);
    }
    return;
  }
  if (Array.isArray(s.oneOf)) {
    const hits = s.oneOf.filter((sub) => subErrors(value, sub, root, path).length === 0);
    if (hits.length !== 1) {
      errors.push(`${path}: matches ${hits.length} oneOf branches, expected exactly 1`);
    }
    return;
  }
  if (Array.isArray(s.allOf)) {
    for (const sub of s.allOf) validate(value, sub, root, path, errors);
  }

  if ("const" in s) {
    if (!deepEqual(value, s.const)) errors.push(`${path}: not equal to const`);
    return;
  }
  if (Array.isArray(s.enum)) {
    if (!s.enum.some((e) => deepEqual(value, e))) {
      errors.push(`${path}: ${JSON.stringify(value)} not in enum`);
    }
    return;
  }

  const type = s.type;
  if (typeof type === "string") {
    if (!typeMatches(value, type)) {
      errors.push(`${path}: expected type ${type}, got ${describe(value)}`);
      return;
    }
    if (type === "string" && typeof s.pattern === "string") {
      if (!new RegExp(s.pattern, "u").test(/** @type {string} */ (value))) {
        errors.push(`${path}: string does not match pattern ${s.pattern}`);
      }
    }
    if (type === "object") validateObject(value, s, root, path, errors);
    if (type === "array") validateArray(value, s, root, path, errors);
    if (type === "integer" || type === "number") {
      const n = /** @type {number} */ (value);
      if (typeof s.minimum === "number" && n < s.minimum) {
        errors.push(`${path}: ${n} < minimum ${s.minimum}`);
      }
      if (typeof s.maximum === "number" && n > s.maximum) {
        errors.push(`${path}: ${n} > maximum ${s.maximum}`);
      }
    }
  } else if (Array.isArray(type)) {
    if (!type.some((t) => typeMatches(value, /** @type {string} */ (t)))) {
      errors.push(`${path}: expected one of types ${type.join("/")}, got ${describe(value)}`);
    }
  } else if (s.properties || s.additionalProperties !== undefined || s.required) {
    validateObject(value, s, root, path, errors);
  }

  const known = new Set([
    "$ref", "$defs", "$schema", "$id", "type", "properties", "required",
    "additionalProperties", "items", "anyOf", "oneOf", "allOf", "enum",
    "const", "pattern", "description", "title", "format", "minimum", "maximum",
  ]);
  for (const key of Object.keys(s)) {
    if (!known.has(key)) {
      errors.push(`${path}: validator does not understand schema keyword ${key} (refusing to silently skip it)`);
    }
  }
}

/**
 * @param {unknown} value
 * @param {Record<string, unknown>} s
 * @param {Record<string, unknown>} root
 * @param {string} path
 * @param {string[]} errors
 */
function validateObject(value, s, root, path, errors) {
  const obj = /** @type {Record<string, unknown>} */ (value);
  const props = /** @type {Record<string, unknown> | undefined} */ (s.properties);
  const required = Array.isArray(s.required) ? s.required : [];
  for (const name of required) {
    if (!(name in obj)) errors.push(`${path}: missing required property ${name}`);
  }
  if (props) {
    for (const [name, sub] of Object.entries(props)) {
      if (name in obj) validate(obj[name], sub, root, `${path}.${name}`, errors);
    }
  }
  const additional = s.additionalProperties;
  if (additional === false) {
    for (const key of Object.keys(obj)) {
      if (!props || !(key in props)) {
        errors.push(`${path}: unknown property ${key} (additionalProperties=false)`);
      }
    }
  } else if (additional && typeof additional === "object") {
    for (const key of Object.keys(obj)) {
      if (!props || !(key in props)) {
        validate(obj[key], additional, root, `${path}.${key}`, errors);
      }
    }
  }
}

/**
 * @param {unknown} value
 * @param {Record<string, unknown>} s
 * @param {Record<string, unknown>} root
 * @param {string} path
 * @param {string[]} errors
 */
function validateArray(value, s, root, path, errors) {
  const arr = /** @type {unknown[]} */ (value);
  if (s.items !== undefined) {
    arr.forEach((item, i) => validate(item, s.items, root, `${path}[${i}]`, errors));
  }
}

/**
 * @param {unknown} value
 * @param {string} type
 */
function typeMatches(value, type) {
  switch (type) {
    case "string": return typeof value === "string";
    case "integer": return Number.isSafeInteger(value);
    case "number": return typeof value === "number" && Number.isFinite(value);
    case "boolean": return typeof value === "boolean";
    case "null": return value === null;
    case "array": return Array.isArray(value);
    case "object": return typeof value === "object" && value !== null && !Array.isArray(value);
    default: return false;
  }
}

/**
 * @param {string} ref
 * @param {Record<string, unknown>} root
 */
function resolveRef(ref, root) {
  const prefix = "#/$defs/";
  if (!ref.startsWith(prefix)) return undefined;
  const name = ref.slice(prefix.length);
  const defs = /** @type {Record<string, unknown> | undefined} */ (root.$defs);
  return defs ? defs[name] : undefined;
}

/** @param {unknown} value @param {unknown} schema @param {Record<string, unknown>} root @param {string} path */
function subErrors(value, schema, root, path) {
  const errors = [];
  validate(value, schema, root, path, errors);
  return errors;
}

/** @param {unknown} a @param {unknown} b */
function deepEqual(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** @param {unknown} v */
function describe(v) {
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  return typeof v;
}
