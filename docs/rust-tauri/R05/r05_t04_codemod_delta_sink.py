#!/usr/bin/env python3
"""R05-T04 mechanical codemod: TurnProviderPort::next_turn gains the live
delta sink (`deltas: &'a dyn TurnDeltaSink`).

Transforms the 30 lingxi-service test doubles (33 impls):
1. `fn next_turn<'a>(...)` impl signatures gain a trailing
   `_deltas: &'a dyn TurnDeltaSink,` parameter line (doubles do not emit;
   the underscore keeps clippy clean).
2. `TurnDeltaSink` joins the file's existing `use lingxi_kernel::ports::{...}`
   list (uniform across the tree; no file lacks it — every double file
   imports TurnProviderPort from there).

The one production call site (runs.rs) and the one production impl
(adapters provider.rs) are hand-edited — this script touches tests only.
"""
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[3]
TESTS = ROOT / "rust" / "crates" / "lingxi-service" / "tests"

changed = []


def process(path: pathlib.Path) -> None:
    text = path.read_text()
    orig = text

    # ── 1) next_turn signatures: insert the sink parameter before `) ->` ────
    out = []
    cursor = 0
    while True:
        m = re.search(r"fn next_turn<'a>\(", text[cursor:])
        if not m:
            out.append(text[cursor:])
            break
        sig_start = cursor + m.start()
        out.append(text[cursor:sig_start])
        sig_end = text.index(") ->", sig_start)
        sig = text[sig_start:sig_end]
        if "TurnDeltaSink" in sig:
            out.append(sig)
            cursor = sig_end
            continue
        lines = sig.split("\n")
        # Parameter indentation follows the existing parameter lines.
        param_indent = next(
            (re.match(r"^(\s*)\S", line).group(1) for line in lines[1:] if line.strip()),
            "        ",
        )
        # The signature text ends at `) ->`, so the last element is the
        # whitespace indent of that closing line — insert before it.
        closing_indent = ""
        if lines and not lines[-1].strip():
            closing_indent = lines.pop()
        lines.append(f"{param_indent}_deltas: &'a dyn TurnDeltaSink,")
        if closing_indent:
            lines.append(closing_indent)
        out.append("\n".join(lines))
        cursor = sig_end
    text = "".join(out)

    # ── 2) import: TurnDeltaSink joins the ports use-list ───────────────────
    if "TurnDeltaSink" in text and "TurnDeltaSink," not in text.split("fn next_turn")[0]:
        # Add to the existing `use lingxi_kernel::ports::{ ... };` block.
        m = re.search(
            r"use lingxi_kernel::ports::\{([^}]*)\};", text, flags=re.S
        )
        if m and "TurnDeltaSink" not in m.group(1):
            names = [n.strip() for n in m.group(1).split(",") if n.strip()]
            names.append("TurnDeltaSink")
            names.sort()
            # Keep the file's brace style: single-line if it was single-line.
            if "\n" in m.group(1):
                body = "\n    " + ",\n    ".join(names) + ",\n"
                text = (
                    text[: m.start()]
                    + "use lingxi_kernel::ports::{"
                    + body
                    + "};"
                    + text[m.end():]
                )
            else:
                text = (
                    text[: m.start()]
                    + "use lingxi_kernel::ports::{"
                    + ", ".join(names)
                    + "};"
                    + text[m.end():]
                )

    if text != orig:
        path.write_text(text)
        changed.append(path.name)


for path in sorted(TESTS.glob("*.rs")):
    process(path)

print(f"rewrote {len(changed)} files")
for name in changed:
    print(f"  {name}")
