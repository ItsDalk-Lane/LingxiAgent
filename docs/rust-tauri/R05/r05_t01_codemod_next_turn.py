#!/usr/bin/env python3
"""R05-T01 mechanical codemod: TurnProviderPort::next_turn typed-input evolution.

Transforms the 29 lingxi-service test doubles:
1. `fn next_turn` impls: drop the `turn: u32` param line, retarget
   `input: &'a str` to `input: &'a ModelTurnInput`, and insert shadowing
   `let turn = input.turn;` / `let input = input.submission.as_str();`
   bindings at the body start when the original (non-underscore) parameter
   name implies the body uses it (the tree is clippy-clean, so a
   non-underscore parameter is always used).
2. `ProviderTurn::ToolRequests { requests ... }` constructions/patterns gain
   `content: Vec::new(),`.
3. Adds the `ModelTurnInput` import where the new signature landed.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
TESTS = ROOT / "rust" / "crates" / "lingxi-service" / "tests"

changed = []


def find_body_open(text: str, start: int) -> int:
    """Index of the `{` opening the fn body after the signature at `start`."""
    idx = text.index(") ->", start)
    return text.index("{", idx)


def process(path: pathlib.Path) -> None:
    text = path.read_text()
    orig = text

    # ── 1) next_turn signatures ────────────────────────────────────────────
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

        turn_name = None
        input_name = None
        input_indent = None
        kept_lines = []
        for line in sig.split("\n"):
            mt = re.match(r"^(\s*)(_?turn): u32,$", line)
            if mt:
                turn_name = mt.group(2)
                continue
            mi = re.match(r"^(\s*)(_?input): &'a str,$", line)
            if mi:
                input_indent, input_name = mi.group(1), mi.group(2)
                kept_lines.append(f"{input_indent}{input_name}: &'a ModelTurnInput,")
                continue
            kept_lines.append(line)
        new_sig = "\n".join(kept_lines)

        body_open = find_body_open(text, sig_start)
        after = text[body_open:]  # starts with '{'
        bindings = []
        # Body indent: fn is at 4 (impl block), body statements at 8.
        fn_indent = re.search(r"(\s*)fn next_turn", new_sig).group(1)
        body_indent = fn_indent + "    "
        if turn_name == "turn":
            bindings.append(f"{body_indent}let turn = input.turn;")
        if input_name == "input":
            bindings.append(f"{body_indent}let input = input.submission.as_str();")
        insert = "\n" + "\n".join(bindings) if bindings else ""

        out.append(new_sig + text[sig_end:body_open] + "{" + insert)
        cursor = body_open + 1
    text = "".join(out)

    # ── 2) ToolRequests constructions ──────────────────────────────────────
    text = re.sub(
        r"ProviderTurn::ToolRequests \{ requests \}",
        "ProviderTurn::ToolRequests {\n"
        "            requests,\n"
        "            content: Vec::new(),\n"
        "        }",
        text,
    )

    def add_content(match: re.Match) -> str:
        line = match.group(0)
        indent = re.match(r"\s*", line).group(0)
        return line + f"\n{indent}    content: Vec::new(),"

    text = re.sub(r"[ \t]*ProviderTurn::ToolRequests \{\n", add_content, text)

    # ── 3) import ──────────────────────────────────────────────────────────
    if "ModelTurnInput" in text and "use lingxi_kernel::model_exchange::ModelTurnInput;" not in text:
        m = re.search(r"^use lingxi_kernel::", text, flags=re.M)
        if m:
            text = (
                text[: m.start()]
                + "use lingxi_kernel::model_exchange::ModelTurnInput;\n"
                + text[m.start():]
            )
        else:
            print(f"WARN: {path.name}: no lingxi_kernel use found", file=sys.stderr)

    if text != orig:
        path.write_text(text)
        changed.append(path.name)


for path in sorted(TESTS.glob("*.rs")):
    process(path)

print(f"rewrote {len(changed)} files")
for name in changed:
    print(f"  {name}")
