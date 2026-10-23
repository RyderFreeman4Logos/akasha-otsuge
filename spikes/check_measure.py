#!/usr/bin/env python3
"""A disappearing process may deny procfs reads; a live process must not be hidden."""

import unittest
from unittest.mock import patch

import measure


class ProcfsExitRace(unittest.TestCase):
    def denied(self, state):
        def read(path):
            if path.name == "stat":
                return f"101 (probe) {state} 0 0 0"
            raise PermissionError("procfs denied during exit")
        return read

    def test_permission_denial_after_exit_is_an_unavailable_terminal_sample(self):
        for function in (measure.process_io, measure.memory):
            for state in ("Z", "X"):
                with self.subTest(function=function.__name__, state=state):
                    with patch("pathlib.Path.read_text", self.denied(state)):
                        result = function(101)
                    self.assertIn(result, (None, {"Rss":0,"Pss":0}))

    def test_permission_denial_for_live_process_remains_an_error(self):
        for function in (measure.process_io, measure.memory):
            with self.subTest(function=function.__name__):
                with patch("pathlib.Path.read_text", self.denied("S")):
                    with self.assertRaises(PermissionError):
                        function(101)

    def test_transient_procfs_denial_recovers_without_hiding_live_errors(self):
        for function, content, expected in (
            (measure.process_io, "rchar: 24\nwrite_bytes: 4096\n", {"rchar":24,"write_bytes":4096}),
            (measure.memory, "Rss: 8 kB\nPss: 4 kB\n", {"Rss":8192,"Pss":4096}),
        ):
            denied_once = False
            def read(path):
                nonlocal denied_once
                if path.name == "stat":
                    return "101 (probe) S 0 0 0"
                if not denied_once:
                    denied_once = True
                    raise PermissionError("transient procfs credential transition")
                return content
            with self.subTest(function=function.__name__):
                with patch("pathlib.Path.read_text", read):
                    self.assertEqual(function(101), expected)

    def test_disappeared_process_and_valid_io(self):
        with patch("pathlib.Path.read_text", side_effect=FileNotFoundError):
            self.assertIsNone(measure.process_io(101))
            self.assertEqual(measure.memory(101), {"Rss":0,"Pss":0})
        with patch("pathlib.Path.read_text", return_value="rchar: 24\nwrite_bytes: 4096\n"):
            self.assertEqual(measure.process_io(101), {"rchar":24,"write_bytes":4096})


if __name__ == "__main__":
    unittest.main()
