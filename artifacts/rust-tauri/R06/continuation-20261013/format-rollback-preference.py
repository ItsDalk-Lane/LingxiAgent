"""Apply rustfmt only to the functions changed for workspace rollback preferences."""
import importlib.util
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
targets = {
    "rust/crates/lingxi-service/src/memory.rs": [
        "rollback_preferences", "rollback_file_changes", "save_rollback_file_changes",
    ],
    "rust/crates/lingxi-service/src/lib.rs": [
        "save_rollback_file_changes", "workspace_rollback_get_route", "workspace_rollback_put_route",
    ],
    "rust/crates/lingxi-service/tests/r06_original_session_proofs.rs": [
        "start_server_at", "stop_and_retain", "stop_and_clean",
        "original_workspace_rollback_preference_persists_without_restoring_files_and_controls_real_gate",
        "original_workspace_rollback_preference_refusals_keep_saved_and_live_state_and_retry",
    ],
    "rust/crates/lingxi-service/tests/r06_ar_f02_file_restore.rs": [
        "ar_f02_c03_second_competitor_preserves_both_external_objects",
    ],
    "rust/crates/lingxi-service/src/skills.rs": [
        "patch_bundle_enabled", "agent_skill_bundle_patch_route",
    ],
    "rust/crates/lingxi-service/tests/r06_original_skills_bundle_proofs.rs": [
        "original_bundle_enable_disable_apply_visible_delta_and_preserve_unrelated_selection",
        "original_bundle_toggle_refusals_and_backing_storage_failures_preserve_selection",
    ],
    "rust/crates/lingxi-service/tests/r06_original_resource_proofs.rs": [
        "start_with_workspace", "start_with_request_limit",
        "original_resource_watch_handle_lifecycle_and_cursor_reads_use_real_provider_registrations",
        "original_resource_events_since_and_stale_resync_follow_real_writes_without_read_side_effects",
    ],
}
for relative, functions in targets.items():
    path = anchors.ROOT / relative
    raw = path.read_text()
    for name in functions:
        declaration = re.search(
            r"(?m)^([ \t]*)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+" + re.escape(name) + r"\b",
            raw,
        )
        if declaration is None:
            raise ValueError("Missing selected function: " + name)
        level = 0
        opened = False
        for token in anchors.tokens(raw[declaration.start():]):
            if token.group() == "{":
                opened = True
                level += 1
            elif token.group() == "}":
                level -= 1
                if opened and level == 0:
                    end = declaration.start() + token.end()
                    break
        else:
            raise ValueError("Missing selected function end: " + name)
        body = raw[declaration.start():end]
        wrapped = bool(declaration.group(1))
        input_text = "impl FormatFragment {\n" + body + "\n}\n" if wrapped else body + "\n"
        output = subprocess.run(
            [str(Path.home() / ".cargo/bin/rustfmt"), "--edition", "2021", "--emit", "stdout"],
            input=input_text, text=True, capture_output=True, check=True,
        ).stdout
        formatted = "\n".join(output.splitlines()[1:-1]) if wrapped else output.rstrip("\n")
        raw = raw[:declaration.start()] + formatted + raw[end:]
    path.write_text(raw)
    print(relative)
