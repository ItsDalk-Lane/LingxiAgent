#!/usr/bin/env python3
"""候选和现行工作树反例只在 /tmp Git 夹具上运行。"""

import hashlib
import json
import pathlib
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

import build_candidate_summary_r2 as build
import verify_candidate_r2 as verify
import worktree_verify_r2 as worktree_verify
import worktree_current


class CandidateRegression(unittest.TestCase):
    def test_candidate_and_current_worktree_changes_are_rejected(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = pathlib.Path(directory)
            docs = root / "docs/rust-tauri/R00"
            out = root / "artifacts/rust-tauri/R00/T01"
            shutil.copytree(build.SCOPE[0], docs)
            shutil.copytree(build.SCOPE[1], out)
            old = root / worktree_current.OLD
            new = root / worktree_current.NEW
            old.mkdir(); new.mkdir()
            original_old = old / "tracked.txt"
            original_old.write_text("tracked original")
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root).decode().strip()
            git("init", "-qb", "main")
            git("add", worktree_current.OLD)
            subprocess.check_call(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                                   "commit", "-qm", "fixture"], cwd=root)
            original_old.unlink()
            new_file = new / "task.txt"
            new_file.write_text("original new taskbook")
            current = worktree_current.collect(root)
            before = {"candidate_sha": current["head"], "branch": current["branch"],
                      "old_tracked_count": len(current["old_tracked_paths"]),
                      "old_deleted_count": len(current["old_deleted_paths"]),
                      "old_diff_sha256": current["old_diff_sha256"],
                      "new_untracked_files": current["new_files"]}
            (out / "worktree-before.json").write_text(json.dumps(before))
            with mock.patch.object(build, "ROOT", root), mock.patch.object(build, "OUT", out), mock.patch.object(build, "SCOPE", (docs, out)), \
                 mock.patch.object(verify, "ROOT", root), mock.patch.object(verify, "OUT", out), \
                 mock.patch.object(verify, "SCOPE", (docs, out)), \
                 mock.patch.object(worktree_verify, "ROOT", root), mock.patch.object(worktree_verify, "OUT", out):
                self.assertEqual(worktree_verify.main(), 0)
                for case in ("R00-A01", "R00-A02"):
                    path = out / f"{case}.result.json"
                    result = json.loads(path.read_text())
                    result["tested_sha"] = current["head"]
                    if case == "R00-A01":
                        result["status"] = "BLOCKED"
                        result["blocked_reason"] = "Synthetic fixture does not run A01"
                    else:
                        result["status"] = "PASS"
                        result["observed"] = json.loads((out / "worktree-preservation-r2.json").read_text())
                    for item in result["evidence"]:
                        item["sha256"] = hashlib.sha256((root / item["path"]).read_bytes()).hexdigest()
                    path.write_text(json.dumps(result))
                build.main()
                self.assertTrue(verify.verify()["ok"], verify.verify()["errors"])
                extra = out / "unexpected.txt"
                extra.write_text("new")
                self.assertFalse(verify.verify()["ok"])
                extra.unlink()
                target = out / "probe-regression-r3.txt"
                original = target.read_bytes()
                target.unlink()
                self.assertFalse(verify.verify()["ok"])
                target.write_bytes(original + b"changed")
                self.assertFalse(verify.verify()["ok"])
                target.write_bytes(original)
                self.assertTrue(verify.verify()["ok"])

                new_file.write_text("changed content")
                self.assertFalse(verify.verify()["ok"])
                new_file.write_text("original new taskbook")
                extra_new = new / "extra.txt"
                extra_new.write_text("new file")
                self.assertFalse(verify.verify()["ok"])
                extra_new.unlink()
                new_file.unlink()
                self.assertFalse(verify.verify()["ok"])
                new_file.write_text("original new taskbook")
                staged = root / "staged.txt"
                staged.write_text("staged")
                git("add", "staged.txt")
                self.assertFalse(verify.verify()["ok"])
                git("reset", "-q", "--", "staged.txt")
                staged.unlink()
                original_old.write_text("tracked original")
                self.assertFalse(verify.verify()["ok"])
                original_old.unlink()
                git("checkout", "-qb", "changed-branch")
                self.assertFalse(verify.verify()["ok"])
                marker = root / "head-marker.txt"
                marker.write_text("new HEAD")
                git("add", "head-marker.txt")
                subprocess.check_call(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                                       "commit", "-qm", "new HEAD"], cwd=root)
                self.assertFalse(verify.verify()["ok"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
