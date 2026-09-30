// R04-T01 / RR-T02-F1 closure evidence: cross-language canonical-JSON key
// order on the non-BMP divergence class.
//
// The TS consumer (tests/migration/r01-t02/canonical-json.mjs) sorts keys
// with Array.prototype.sort() — UTF-16 code-unit order. The Rust writer
// (rust/crates/lingxi-protocol/src/canon.rs) was aligned in R04-T01 to the
// same UTF-16 code-unit order (its golden test
// `non_bmp_keys_sort_in_utf16_unit_order_ts_parity` asserts the identical
// expected literal). This script proves the TS side emits exactly those
// bytes for the same input — the divergence class (astral key vs
// U+E000..=U+FFFF key) no longer forks the two implementations.
//
// Run: node cross-lang-canon-nonbmp.mjs   (exit 0 = parity holds)
import { canonicalStringify } from "../../../../tests/migration/r01-t02/canonical-json.mjs";

const astral = "\u{1F004}"; // surrogate pair 0xD83D 0xDE04
const privateUse = "\u{E000}"; // 0xE000 (private use area)
const ascii = "z";

// Object key insertion order deliberately puts the PUA key FIRST; the
// canonical profile must reorder by UTF-16 units: 'z'(0x7A) < D83D… < E000.
const input = { [privateUse]: 1, [astral]: 2, [ascii]: 3 };
const canonical = canonicalStringify(input);

// The exact literal the Rust golden test asserts (canon.rs,
// non_bmp_keys_sort_in_utf16_unit_order_ts_parity).
const rustExpected = `{"z":3,"${astral}":2,"${privateUse}":1}`;

if (canonical !== rustExpected) {
  console.error("MISMATCH");
  console.error("  ts canonical :", JSON.stringify(canonical));
  console.error("  rust expected:", JSON.stringify(rustExpected));
  process.exit(1);
}
console.log("PARITY OK: ts canonical == rust golden for the non-BMP divergence class");
console.log("  bytes:", JSON.stringify(canonical));
process.exit(0);
