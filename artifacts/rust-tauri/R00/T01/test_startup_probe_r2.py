#!/usr/bin/env python3
"""R00-A01 取证脚本回归：等长篡改、排他创建与只清理自有哨兵。"""

import os
import pathlib
import tempfile
import unittest
from unittest import mock

import startup_probe_r2 as probe


class ProbeRegression(unittest.TestCase):
    def test_creation_failures_clean_only_owned_partial_file(self):
        failures = ("write", "fchmod", "fsync", "fstat_initial", "fstat_final", "readback", "close")
        for stage in failures:
            with self.subTest(stage=stage), tempfile.TemporaryDirectory(dir="/tmp") as directory:
                root = pathlib.Path(directory)
                path = root / ".r00-a01-readonly-fail"
                target = {"write": probe.os.write, "fchmod": probe.os.fchmod, "fsync": probe.os.fsync,
                          "fstat_initial": probe.os.fstat, "fstat_final": probe.os.fstat,
                          "readback": probe.digest_fd, "close": probe.os.close}[stage]
                real_fstat = os.fstat
                calls = 0

                def failing(*args, **kwargs):
                    nonlocal calls
                    fd = args[0]
                    is_sentinel = False
                    # 使用原始 fstat 识别本次哨兵，避开准备阶段的能力探针文件。
                    if path.exists():
                        try:
                            is_sentinel = real_fstat(fd).st_ino == path.stat().st_ino
                        except OSError:
                            is_sentinel = False
                    if is_sentinel:
                        calls += 1
                    should_fail = is_sentinel and calls == (2 if stage == "fstat_final" else 1)
                    if should_fail:
                        if stage == "close":
                            target(*args, **kwargs)
                        raise OSError(28, "injected")
                    return target(*args, **kwargs)

                attribute = "digest_fd" if stage == "readback" else ("fstat" if stage.startswith("fstat") else stage)
                owner = probe if stage == "readback" else probe.os
                with mock.patch.object(owner, attribute, side_effect=failing):
                    with self.assertRaises((OSError, RuntimeError)):
                        probe.create_sentinel(root, "fail")
                self.assertFalse(path.exists())

    def test_zero_and_short_write(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = pathlib.Path(directory)
            original = probe.os.write
            with mock.patch.object(probe.os, "write", return_value=0):
                with self.assertRaises(OSError):
                    probe.create_sentinel(root, "zero")
            self.assertFalse((root / ".r00-a01-readonly-zero").exists())
            calls = 0

            def short(fd, data):
                nonlocal calls
                calls += 1
                return original(fd, data[:1])

            with mock.patch.object(probe.os, "write", side_effect=short):
                path, created = probe.create_sentinel(root, "short")
            self.assertGreater(calls, 1)
            self.assertTrue(probe.delete_owned_sentinel(path, created)["removed"])

    def test_second_directory_failure_cleans_both(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            roots = [pathlib.Path(directory) / name for name in ("one", "two")]
            for root in roots:
                root.mkdir()
            owned = []
            try:
                owned.append(probe.create_sentinel(roots[0], "pair"))
                with mock.patch.object(probe.os, "fsync", side_effect=OSError(28, "injected")):
                    with self.assertRaises(OSError):
                        probe.create_sentinel(roots[1], "pair")
            finally:
                for path, created in owned:
                    self.assertTrue(probe.delete_owned_sentinel(path, created)["removed"])
            for root in roots:
                self.assertFalse((root / ".r00-a01-readonly-pair").exists())

    def test_replaced_directory_is_not_followed(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            base = pathlib.Path(directory)
            root, outside = base / "root", base / "outside"
            root.mkdir(); outside.mkdir()
            (root / "child").mkdir()
            (outside / "secret").write_text("outside")
            original = probe.os.open
            swapped = False

            def replace_child(name, flags, *args, **kwargs):
                nonlocal swapped
                if name == "child" and not swapped:
                    swapped = True
                    (root / "child").rename(root / "old-child")
                    (root / "child").symlink_to(outside, target_is_directory=True)
                return original(name, flags, *args, **kwargs)

            with mock.patch.object(probe.os, "open", side_effect=replace_child):
                result = probe.tree_digest(root)
            self.assertGreater(result["errors"], 0)
            self.assertEqual(result["files"], 0)

    def test_replaced_file_is_not_read(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = pathlib.Path(directory) / "root"
            root.mkdir()
            target = root / "entry"
            target.write_text("old")
            original = probe.os.open
            swapped = False

            def replace_file(name, flags, *args, **kwargs):
                nonlocal swapped
                if name == "entry" and not swapped:
                    swapped = True
                    target.rename(root / "old-entry")
                    target.write_text("new")
                return original(name, flags, *args, **kwargs)

            with mock.patch.object(probe.os, "open", side_effect=replace_file):
                result = probe.tree_digest(root)
            self.assertGreater(result["errors"], 0)

    def test_attempt_aggregation_retains_same_window_failure(self):
        attempts = [{"environment_epoch": "one", "status": "BLOCKED", "quiet_window": False, "cleanup_safe": True,
                     "all_checks": False},
                    {"environment_epoch": "one", "status": "OBSERVED_STABLE", "quiet_window": True,
                     "cleanup_safe": True, "all_checks": True}]
        self.assertEqual(probe.aggregate_attempts(attempts, "one"), "BLOCKED")
        self.assertEqual(probe.aggregate_attempts(attempts + [{"environment_epoch": "two",
            "status": "OBSERVED_STABLE", "quiet_window": True, "cleanup_safe": True, "all_checks": True}], "two"), "PASS")
    def test_equal_length_edit_with_preserved_mtime_changes_content_digest(self):
        with tempfile.TemporaryDirectory(prefix="r00-probe-regression-") as directory:
            root = pathlib.Path(directory)
            target = root / "sample"
            target.write_bytes(b"one")
            before = probe.tree_digest(root)
            original = target.stat()
            target.write_bytes(b"two")
            os.utime(target, ns=(original.st_atime_ns, original.st_mtime_ns))
            after = probe.tree_digest(root)
            self.assertEqual(before["files"], after["files"])
            self.assertEqual(before["bytes"], after["bytes"])
            self.assertNotEqual(before["content_sha256"], after["content_sha256"])

    def test_sentinel_is_exclusive_readonly_and_only_original_is_removed(self):
        with tempfile.TemporaryDirectory(prefix="r00-probe-regression-") as directory:
            root = pathlib.Path(directory)
            path, created = probe.create_sentinel(root, "fixed-run-id")
            self.assertEqual(created["mode"], "0o400")
            self.assertEqual(probe.sentinel_state(path)["sha256"], created["sha256"])
            with self.assertRaises(FileExistsError):
                probe.create_sentinel(root, "fixed-run-id")
            self.assertTrue(probe.delete_owned_sentinel(path, created)["removed"])
            self.assertFalse(path.exists())

    def test_changed_sentinel_is_not_cleaned(self):
        with tempfile.TemporaryDirectory(prefix="r00-probe-regression-") as directory:
            root = pathlib.Path(directory)
            path, created = probe.create_sentinel(root, "fixed-run-id")
            path.chmod(0o600)
            path.write_bytes(b"other")
            self.assertFalse(probe.delete_owned_sentinel(path, created)["removed"])
            self.assertTrue(path.exists())

    def test_replaced_sentinel_link_and_parent_are_not_deleted(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            base = pathlib.Path(directory)
            root, outside = base / "root", base / "outside"
            root.mkdir(); outside.mkdir()
            path, created = probe.create_sentinel(root, "replace")
            old = root / "original"
            path.rename(old)
            target = outside / "untouched"
            target.write_text("user owned")
            path.symlink_to(target)
            self.assertFalse(probe.delete_owned_sentinel(path, created)["removed"])
            self.assertEqual(target.read_text(), "user owned")
            self.assertTrue(old.exists())
            path.unlink()
            old.rename(path)
            moved = base / "moved"
            root.rename(moved)
            root.symlink_to(outside, target_is_directory=True)
            self.assertFalse(probe.delete_owned_sentinel(path, created)["removed"])
            self.assertTrue((moved / path.name).exists())
            self.assertEqual(target.read_text(), "user owned")


if __name__ == "__main__":
    unittest.main(verbosity=2)
