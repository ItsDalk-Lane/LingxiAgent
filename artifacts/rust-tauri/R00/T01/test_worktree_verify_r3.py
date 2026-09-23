#!/usr/bin/env python3
"""A02 辅助校验反例：暂存差异与原有未跟踪文件变化均拒绝。"""

import hashlib
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

import worktree_verify_r2 as verify


class WorktreeRegression(unittest.TestCase):
    def test_staged_and_untracked_changes_fail(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = pathlib.Path(directory)
            old = root / verify.OLD
            new = root / verify.NEW
            out = root / "evidence"
            old.mkdir(); new.mkdir(); out.mkdir()
            (old / "tracked.txt").write_text("original")

            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root).decode().strip()

            git("init", "-q")
            git("add", verify.OLD)
            subprocess.check_call(["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                                   "commit", "-qm", "fixture"], cwd=root)
            (old / "tracked.txt").unlink()
            untracked = new / "fixture.txt"
            untracked.write_text("untouched")
            original = {"candidate_sha": git("rev-parse", "HEAD"), "branch": git("branch", "--show-current"),
                        "old_tracked_count": 1, "old_deleted_count": 1,
                        "old_diff_sha256": hashlib.sha256(subprocess.check_output(
                            ["git", "diff", "--binary", "--", verify.OLD], cwd=root)).hexdigest(),
                        "new_untracked_files": {str(untracked.relative_to(root)): hashlib.sha256(untracked.read_bytes()).hexdigest()}}
            (out / "worktree-before.json").write_text(json.dumps(original))
            with mock.patch.object(verify, "ROOT", root), mock.patch.object(verify, "OUT", out):
                self.assertEqual(verify.main(), 0)
                (root / "staged.txt").write_text("staged")
                git("add", "staged.txt")
                self.assertEqual(verify.main(), 1)
                staged = json.loads((out / "worktree-preservation-r2.json").read_text())
                self.assertFalse(staged["checks"]["staged_diff_absent"])
                git("reset", "-q", "--", "staged.txt")
                (root / "staged.txt").unlink()
                untracked.write_text("changed")
                self.assertEqual(verify.main(), 1)
                changed = json.loads((out / "worktree-preservation-r2.json").read_text())
                self.assertFalse(changed["checks"]["new_untracked_files_unchanged"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
