#!/usr/bin/env python3
"""R3 发现项的 /tmp 故障注入；不访问真实 Lingxi 目录。"""

import errno
import os
import pathlib
import tempfile
import unittest
from unittest import mock

import startup_probe_r2 as probe


class AtomicCleanupRegression(unittest.TestCase):
    def _root(self, base, name="root"):
        root = pathlib.Path(base) / name
        root.mkdir()
        return root

    def test_last_parent_close_before_and_after_effect_in_both_roots(self):
        for root_number in (0, 1):
            for after_effect in (False, True):
                with self.subTest(root_number=root_number, after_effect=after_effect), tempfile.TemporaryDirectory(dir="/tmp") as directory:
                    roots = [self._root(directory, "one"), self._root(directory, "two")]
                    owned = []
                    original_close = os.close
                    injected = False
                    target_inode = roots[root_number].stat().st_ino

                    def fail_last_parent_close(fd):
                        nonlocal injected
                        try:
                            identity = os.fstat(fd)
                        except OSError:
                            return original_close(fd)
                        # 创建阶段首次关闭根目录同身份 FD，是最后的父目录关闭。
                        if identity.st_ino == target_inode and not injected:
                            injected = True
                            if after_effect:
                                original_close(fd)
                            raise OSError(errno.EIO, "injected final parent close")
                        return original_close(fd)

                    try:
                        if root_number == 1:
                            owned.append(probe.create_sentinel(roots[0], "pair"))
                        with mock.patch.object(probe.os, "close", side_effect=fail_last_parent_close):
                            with self.assertRaises((OSError, RuntimeError)):
                                probe.create_sentinel(roots[root_number], "pair")
                    finally:
                        for path, created in owned:
                            self.assertTrue(probe.delete_owned_sentinel(path, created)["removed"])
                    self.assertTrue(injected)
                    for root in roots:
                        self.assertFalse((root / ".r00-a01-readonly-pair").exists())
                        self.assertEqual(list(root.iterdir()), [])

    def test_foreign_regular_and_link_swapped_at_atomic_move_survive(self):
        for kind in ("regular", "link"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(dir="/tmp") as directory:
                root = self._root(directory)
                path, created = probe.create_sentinel(root, "swap")
                saved = pathlib.Path(directory) / "saved-original"
                target = pathlib.Path(directory) / "link-target"
                target.write_bytes(b"target-stays")
                original_move = probe.rename_exclusive
                changed = False

                def swap_then_move(from_fd, from_name, to_fd, to_name):
                    nonlocal changed
                    if from_name == path.name and not changed:
                        changed = True
                        path.rename(saved)
                        if kind == "regular":
                            path.write_bytes(b"foreign-stays")
                        else:
                            path.symlink_to(target)
                    return original_move(from_fd, from_name, to_fd, to_name)

                with mock.patch.object(probe, "rename_exclusive", side_effect=swap_then_move):
                    outcome = probe.delete_owned_sentinel(path, created)
                self.assertFalse(outcome["removed"])
                self.assertTrue(saved.exists())
                self.assertEqual(target.read_bytes(), b"target-stays")
                self.assertTrue(path.exists() or path.is_symlink())
                if kind == "regular":
                    self.assertEqual(path.read_bytes(), b"foreign-stays")
                else:
                    self.assertTrue(path.is_symlink())
                self.assertTrue(outcome.get("foreign_restored"))

    def test_foreign_moved_but_source_recreated_is_retained(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            path, created = probe.create_sentinel(root, "blocked-restore")
            saved = pathlib.Path(directory) / "saved-original"
            original_move = probe.rename_exclusive
            changed = False

            def swap_and_recreate(from_fd, from_name, to_fd, to_name):
                nonlocal changed
                if from_name == path.name and not changed:
                    changed = True
                    path.rename(saved)
                    path.write_bytes(b"foreign-held")
                    original_move(from_fd, from_name, to_fd, to_name)
                    path.write_bytes(b"foreign-new")
                    return
                return original_move(from_fd, from_name, to_fd, to_name)

            with mock.patch.object(probe, "rename_exclusive", side_effect=swap_and_recreate):
                outcome = probe.delete_owned_sentinel(path, created)
            self.assertFalse(outcome["removed"])
            self.assertEqual(path.read_bytes(), b"foreign-new")
            self.assertEqual(saved.read_bytes(), probe.SENTINEL_BODY)
            quarantine = pathlib.Path(outcome["quarantine_path"])
            self.assertEqual(len(list(quarantine.iterdir())), 1)
            self.assertEqual(next(quarantine.iterdir()).read_bytes(), b"foreign-held")

    def test_exclusive_move_errors_and_target_preoccupied_fail(self):
        for error_code in (errno.EXDEV, errno.ENOTSUP, errno.EEXIST):
            with self.subTest(error_code=error_code), tempfile.TemporaryDirectory(dir="/tmp") as directory:
                root = self._root(directory)
                path, created = probe.create_sentinel(root, "move-error")
                original_move = probe.rename_exclusive
                def failing(from_fd, from_name, to_fd, to_name):
                    if from_name == path.name:
                        raise OSError(error_code, "injected")
                    return original_move(from_fd, from_name, to_fd, to_name)
                with mock.patch.object(probe, "rename_exclusive", side_effect=failing):
                    outcome = probe.delete_owned_sentinel(path, created)
                self.assertFalse(outcome["removed"])
                self.assertTrue(path.exists())

    def test_move_after_effect_error_and_unlink_after_effect_error_fail(self):
        for point in ("move", "unlink"):
            with self.subTest(point=point), tempfile.TemporaryDirectory(dir="/tmp") as directory:
                root = self._root(directory)
                path, created = probe.create_sentinel(root, point)
                if point == "move":
                    original = probe.rename_exclusive
                    def after_move(from_fd, from_name, to_fd, to_name):
                        original(from_fd, from_name, to_fd, to_name)
                        if from_name == path.name:
                            raise OSError(errno.EIO, "after move")
                    patcher = mock.patch.object(probe, "rename_exclusive", side_effect=after_move)
                else:
                    original = os.unlink
                    def after_unlink(name, *args, **kwargs):
                        original(name, *args, **kwargs)
                        if name.startswith("held-"):
                            raise OSError(errno.EIO, "after unlink")
                    patcher = mock.patch.object(probe.os, "unlink", side_effect=after_unlink)
                with patcher:
                    outcome = probe.delete_owned_sentinel(path, created)
                self.assertFalse(outcome["removed"])
                self.assertFalse(path.exists())
                self.assertEqual(list(root.iterdir()), []) if point == "move" else None

    def test_source_recreated_after_owned_move_fails_without_deleting_foreign(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            path, created = probe.create_sentinel(root, "recreated")
            original = probe.rename_exclusive
            def after_move(from_fd, from_name, to_fd, to_name):
                original(from_fd, from_name, to_fd, to_name)
                if from_name == path.name:
                    path.write_bytes(b"foreign")
            with mock.patch.object(probe, "rename_exclusive", side_effect=after_move):
                outcome = probe.delete_owned_sentinel(path, created)
            self.assertFalse(outcome["removed"])
            self.assertEqual(path.read_bytes(), b"foreign")

    def test_capability_probe_failure_prevents_sentinel_creation(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            with mock.patch.object(probe, "rename_exclusive", side_effect=OSError(errno.ENOTSUP, "unsupported")):
                with self.assertRaises(OSError):
                    probe.create_sentinel(root, "unsupported")
            self.assertEqual(list(root.iterdir()), [])

    def test_capability_file_close_before_and_after_effect_is_cleaned(self):
        for after_effect in (False, True):
            with self.subTest(after_effect=after_effect), tempfile.TemporaryDirectory(dir="/tmp") as directory:
                root = self._root(directory)
                original_close = os.close
                injected = False
                def fail_capability_close(fd):
                    nonlocal injected
                    state = os.fstat(fd)
                    if not injected and state.st_nlink == 1 and state.st_size == 0 and \
                       (state.st_mode & 0o170000) == 0o100000:
                        injected = True
                        if after_effect:
                            original_close(fd)
                        raise OSError(errno.EIO, "capability close")
                    return original_close(fd)
                with mock.patch.object(probe.os, "close", side_effect=fail_capability_close):
                    with self.assertRaises((OSError, RuntimeError)):
                        probe.create_sentinel(root, "capability-close")
                self.assertTrue(injected)
                self.assertEqual(list(root.iterdir()), [])

    def test_creation_failure_uses_same_foreign_safe_cleanup(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            path = root / ".r00-a01-readonly-creation-fail"
            saved = pathlib.Path(directory) / "saved-original"
            original_move = probe.rename_exclusive
            changed = False

            def swap_during_cleanup(from_fd, from_name, to_fd, to_name):
                nonlocal changed
                if from_name == path.name and not changed:
                    changed = True
                    path.rename(saved)
                    path.write_bytes(b"foreign")
                return original_move(from_fd, from_name, to_fd, to_name)

            with mock.patch.object(probe.os, "fsync", side_effect=OSError(errno.EIO, "create failure")), \
                 mock.patch.object(probe, "rename_exclusive", side_effect=swap_during_cleanup):
                with self.assertRaises(RuntimeError):
                    probe.create_sentinel(root, "creation-fail")
            self.assertTrue(changed)
            self.assertEqual(path.read_bytes(), b"foreign")
            self.assertEqual(saved.read_bytes(), probe.SENTINEL_BODY)
            self.assertFalse(probe._CREATE_FAILURES[-1]["cleanup"]["removed"])

    def test_quarantine_replacement_and_occupied_target_fail_closed(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            path, created = probe.create_sentinel(root, "replaced-quarantine")
            lease = probe._LEASES[path]
            quarantine = root / lease["quarantine_name"]
            saved = pathlib.Path(directory) / "saved-quarantine"
            outside = self._root(directory, "outside")
            quarantine.rename(saved)
            quarantine.symlink_to(outside, target_is_directory=True)
            outcome = probe.delete_owned_sentinel(path, created)
            self.assertFalse(outcome["removed"])
            self.assertTrue(path.exists())
            self.assertTrue(quarantine.is_symlink())
            self.assertTrue(saved.is_dir())

        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            path, created = probe.create_sentinel(root, "occupied-target")
            original_move = probe.rename_exclusive
            def occupy_then_move(from_fd, from_name, to_fd, to_name):
                if from_name == path.name:
                    fd = os.open(to_name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=to_fd)
                    os.close(fd)
                return original_move(from_fd, from_name, to_fd, to_name)
            with mock.patch.object(probe, "rename_exclusive", side_effect=occupy_then_move):
                outcome = probe.delete_owned_sentinel(path, created)
            self.assertFalse(outcome["removed"])
            self.assertTrue(path.exists())
            self.assertEqual(len(list(pathlib.Path(outcome["quarantine_path"]).iterdir())), 1)

    def test_rmdir_failure_cannot_report_success(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            path, created = probe.create_sentinel(root, "rmdir-fail")
            original = os.rmdir
            def fail_final_rmdir(name, *args, **kwargs):
                if name.startswith(".r00-a01-quarantine-"):
                    raise OSError(errno.EIO, "injected")
                return original(name, *args, **kwargs)
            with mock.patch.object(probe.os, "rmdir", side_effect=fail_final_rmdir):
                outcome = probe.delete_owned_sentinel(path, created)
            self.assertFalse(outcome["removed"])
            self.assertFalse(outcome["quarantine_removed"])
            self.assertFalse(path.exists())

    def test_same_uid_quarantine_replacement_remains_documented_risk(self):
        # 同 UID 可进入 0700 目录并在核对后换名；此反例记录剩余风险，不当作修复完成。
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            root = self._root(directory)
            path, created = probe.create_sentinel(root, "same-uid")
            quarantine = root / probe._LEASES[path]["quarantine_name"]
            saved = pathlib.Path(directory) / "saved-original"
            original_unlink = os.unlink
            injected = False
            def same_uid_swap(name, *args, **kwargs):
                nonlocal injected
                if name.startswith("held-") and not injected:
                    injected = True
                    (quarantine / name).rename(saved)
                    (quarantine / name).write_bytes(b"same-uid-foreign")
                return original_unlink(name, *args, **kwargs)
            with mock.patch.object(probe.os, "unlink", side_effect=same_uid_swap):
                outcome = probe.delete_owned_sentinel(path, created)
            self.assertTrue(injected)
            self.assertTrue(outcome["removed"])
            self.assertTrue(saved.exists())
            self.assertFalse(quarantine.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
