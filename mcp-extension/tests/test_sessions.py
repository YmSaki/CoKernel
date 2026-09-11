from __future__ import annotations

import unittest

from cokernel_mcp_extension.sessions import (
    SessionResolutionError,
    normalize_notebook_path,
    select_existing_kernel_id,
)


class SessionSelectionTests(unittest.TestCase):
    def test_normalizes_jupyter_paths(self) -> None:
        self.assertEqual(normalize_notebook_path("/notebooks/demo.ipynb"), "notebooks/demo.ipynb")
        self.assertEqual(normalize_notebook_path("notebooks%2Fdemo.ipynb"), "notebooks/demo.ipynb")
        self.assertEqual(normalize_notebook_path(r"notebooks\\demo.ipynb"), "notebooks/demo.ipynb")

    def test_selects_current_session_model(self) -> None:
        sessions = [
            {"id": "s1", "path": "notebooks/demo.ipynb", "kernel": {"id": "kernel-a"}},
            {"id": "s2", "path": "notebooks/other.ipynb", "kernel": {"id": "kernel-b"}},
        ]
        self.assertEqual(select_existing_kernel_id(sessions, "notebooks/demo.ipynb"), "kernel-a")

    def test_supports_legacy_nested_notebook_path(self) -> None:
        sessions = [
            {"id": "s1", "notebook": {"path": "demo.ipynb"}, "kernel": {"id": "kernel-a"}}
        ]
        self.assertEqual(select_existing_kernel_id(sessions, "demo.ipynb"), "kernel-a")

    def test_deduplicates_same_kernel(self) -> None:
        sessions = [
            {"id": "s1", "path": "demo.ipynb", "kernel": {"id": "kernel-a"}},
            {"id": "s2", "path": "demo.ipynb", "kernel": {"id": "kernel-a"}},
        ]
        self.assertEqual(select_existing_kernel_id(sessions, "demo.ipynb"), "kernel-a")

    def test_fails_when_notebook_has_no_running_session(self) -> None:
        with self.assertRaisesRegex(SessionResolutionError, "No running Jupyter session"):
            select_existing_kernel_id([], "demo.ipynb")

    def test_fails_when_notebook_has_multiple_kernels(self) -> None:
        sessions = [
            {"id": "s1", "path": "demo.ipynb", "kernel": {"id": "kernel-a"}},
            {"id": "s2", "path": "demo.ipynb", "kernel": {"id": "kernel-b"}},
        ]
        with self.assertRaisesRegex(SessionResolutionError, "More than one running kernel"):
            select_existing_kernel_id(sessions, "demo.ipynb")


if __name__ == "__main__":
    unittest.main()
