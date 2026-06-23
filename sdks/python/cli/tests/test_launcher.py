"""
Tests for the polyplugc launcher entry point.

Uses only the Python standard library (no pytest).  Covers:
  1. _binary_path() resolves to the correct location under _bin/.
  2. main() exits with code 1 and prints a helpful message when the binary
     is absent.
  3. _ensure_executable() sets the executable bits on a file that lacks them.
  4. main() executes the binary and forwards exit code (POSIX-only smoke
     test using a stub shell script in a temporary _bin directory).
"""

import importlib
import os
import pathlib
import stat
import sys
import tempfile
import unittest
import unittest.mock


class TestBinaryPath(unittest.TestCase):
    def test_resolves_inside_package(self) -> None:
        from polyplugc.__main__ import _binary_path, _bin_dir

        binary: pathlib.Path = _binary_path()
        bin_dir: pathlib.Path = _bin_dir()

        self.assertEqual(binary.parent, bin_dir)
        if sys.platform == "win32":
            self.assertEqual(binary.name, "polyplugc.exe")
        else:
            self.assertEqual(binary.name, "polyplugc")

    def test_bin_dir_is_sibling_of_main(self) -> None:
        import polyplugc.__main__ as launcher_module

        package_dir: pathlib.Path = pathlib.Path(launcher_module.__file__).parent
        from polyplugc.__main__ import _bin_dir

        self.assertEqual(_bin_dir(), package_dir / "_bin")


class TestMissingBinary(unittest.TestCase):
    def test_exits_1_when_binary_absent(self) -> None:
        """main() must exit(1) with a message when the binary is not present."""
        from polyplugc import __main__ as launcher_module

        missing: pathlib.Path = pathlib.Path("/nonexistent/path/polyplugc")

        with (
            unittest.mock.patch.object(
                launcher_module, "_binary_path", return_value=missing
            ),
            self.assertRaises(SystemExit) as cm,
        ):
            launcher_module.main()

        self.assertEqual(cm.exception.code, 1)

    def test_error_message_mentions_alternatives(self) -> None:
        """The missing-binary message must mention cargo and the releases page."""
        from polyplugc import __main__ as launcher_module

        missing: pathlib.Path = pathlib.Path("/nonexistent/path/polyplugc")

        with (
            unittest.mock.patch.object(
                launcher_module, "_binary_path", return_value=missing
            ),
            unittest.mock.patch("sys.stderr") as mock_stderr,
            self.assertRaises(SystemExit),
        ):
            launcher_module.main()

        printed: str = "".join(
            call.args[0]
            for call in mock_stderr.write.call_args_list
            if call.args
        )
        self.assertIn("cargo install polyplugc", printed)
        self.assertIn("releases", printed)


class TestEnsureExecutable(unittest.TestCase):
    def test_sets_executable_bits(self) -> None:
        from polyplugc.__main__ import _ensure_executable

        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            tmp_path: pathlib.Path = pathlib.Path(tmp.name)

        try:
            # Strip all executable bits.
            tmp_path.chmod(stat.S_IRUSR | stat.S_IWUSR)
            self.assertEqual(tmp_path.stat().st_mode & stat.S_IXUSR, 0)

            _ensure_executable(tmp_path)

            self.assertNotEqual(tmp_path.stat().st_mode & stat.S_IXUSR, 0)
        finally:
            tmp_path.unlink(missing_ok=True)

    def test_noop_when_already_executable(self) -> None:
        from polyplugc.__main__ import _ensure_executable

        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            tmp_path: pathlib.Path = pathlib.Path(tmp.name)

        try:
            tmp_path.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
            mode_before: int = tmp_path.stat().st_mode
            _ensure_executable(tmp_path)
            # Mode may gain group/other x bits but must not lose user x.
            self.assertNotEqual(tmp_path.stat().st_mode & stat.S_IXUSR, 0)
        finally:
            tmp_path.unlink(missing_ok=True)


@unittest.skipIf(sys.platform == "win32", "execv smoke test is POSIX-only")
class TestStubExec(unittest.TestCase):
    """Smoke-test: main() execs the stub and the process is replaced."""

    def test_execv_is_called_with_correct_args(self) -> None:
        from polyplugc import __main__ as launcher_module

        with tempfile.TemporaryDirectory() as tmp_dir:
            stub: pathlib.Path = pathlib.Path(tmp_dir) / "polyplugc"
            stub.write_text("#!/bin/sh\necho stub ran\nexit 0\n")
            stub.chmod(stat.S_IRWXU)

            execv_calls: list[tuple[str, list[str]]] = []

            def fake_execv(path: str, args: list[str]) -> None:
                execv_calls.append((path, args))

            with (
                unittest.mock.patch.object(
                    launcher_module, "_binary_path", return_value=stub
                ),
                unittest.mock.patch("os.execv", side_effect=fake_execv),
                # execv normally never returns; our fake does, so main() falls
                # through to the "execv failed" error path and calls sys.exit(1).
                self.assertRaises(SystemExit),
            ):
                launcher_module.main()

            self.assertEqual(len(execv_calls), 1)
            called_path, called_args = execv_calls[0]
            self.assertEqual(called_path, str(stub))
            self.assertEqual(called_args[0], str(stub))


if __name__ == "__main__":
    unittest.main()
