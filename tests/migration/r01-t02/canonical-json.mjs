// Canonical JSON (lingxi-canonical-json-v1) — TypeScript implementation.
//
// Must match rust/crates/lingxi-protocol/src/canon.rs byte-for-byte:
//   - UTF-8, no whitespace, no trailing newline;
//   - object keys sorted by code point (UTF-16 unit order equals code point
//     order, so Array.prototype.sort default is correct here);
//   - non-ASCII emitted raw (JSON.stringify behavior);
//   - integers only. A number that is not a safe integer is a hard error,
//     never a silent precision loss — u64 quantities travel as decimal
//     strings per the protocol spec.

/**
 * @param {unknown} value
 * @returns {string} canonical JSON text
 */
export function canonicalStringify(value) {
  return write(value);
}

/**
 * @param {unknown} value
 * @returns {Buffer} canonical JSON bytes (UTF-8)
 */
export function canonicalBytes(value) {
  return Buffer.from(write(value), "utf-8");
}

/** @param {unknown} value @returns {string} */
function write(value) {
  if (value === null) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value)) {
      throw new Error(
        `canonical-json: non-safe-integer number on the wire is forbidden: ${value}`,
      );
    }
    return String(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(write).join(",")}]`;
  }
  if (typeof value === "object") {
    const obj = /** @type {Record<string, unknown>} */ (value);
    const keys = Object.keys(obj).sort();
    const parts = keys.map((k) => `${JSON.stringify(k)}:${write(obj[k])}`);
    return `{${parts.join(",")}}`;
  }
  throw new Error(`canonical-json: unsupported value type ${typeof value}`);
}
