import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("package_linux", Path(__file__).with_name("package-linux.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class PackageGuideTests(unittest.TestCase):
    def test_only_repository_destinations_in_the_source_footer_change(self):
        body = (
            "[package](adapters.md#install) [here](#start) [external](https://example.com/x)\n"
            "[evidence][source-evidence] ![chart][source-chart] [directory][source-tree]\n"
            "`[example](../missing.txt)`\n> ~~~md\n> [example](../missing.txt)\n> ~~~\n"
            "An unclosed example [example](../missing.txt\n\n`unclosed\n\n[real][source-tree]\n"
            "\n    [a code example](../missing.txt)\n\n[ordinary]: <adapters.md#install>\n"
        )
        definitions = (
            "[source-evidence]: <../tests/receipt.json?q=1#key>\n"
            "[source-chart]: <../tests/chart%20file.png>\n"
            "[source-tree]: <../tests/>\n[source-local-dir]: <adapters/>\n"
        )
        text = body + "\n" + release.SOURCE_LINKS + "\n" + definitions
        members = {"docs/adapters.md", "docs/adapters/pi/README.md"}
        paths = {"tests": "tree", "tests/receipt.json": "blob", "tests/chart file.png": "blob"}
        base = "https://github.com/example/repo"
        actual = release.package_markdown(text, "docs/install.md", members, paths, base, "abc123")
        expected = definitions.replace("../tests/receipt", f"{base}/blob/abc123/tests/receipt")
        expected = expected.replace("../tests/chart", f"{base}/blob/abc123/tests/chart")
        expected = expected.replace("<../tests/>", f"<{base}/tree/abc123/tests>")
        self.assertEqual(actual, body + "\n" + release.SOURCE_LINKS + "\n" + expected)
        padding = "\n  \n"
        self.assertEqual(release.package_markdown(text + padding, "docs/install.md", members, paths, base, "abc123"),
                         actual + padding)
        self.assertEqual(release.package_markdown(body, "docs/install.md", members, paths, base, "abc123"), body)

    def test_claude_guide_publishes_the_native_compatibility_receipt(self):
        guide = "docs/adapters.md"
        text = (release.ROOT / guide).read_text()
        files = {path: "blob" for path in release.output("git", "ls-files").splitlines()}
        base = "https://github.com/RyderFreeman4Logos/akasha-otsuge"
        packed = release.package_markdown(text, guide, {guide}, files, base, "qualified-revision")
        target = "tests/fixtures/claude/2.1.322/README.md"
        self.assertIn(f"<{base}/blob/qualified-revision/{target}>", packed)

    def test_missing_repository_file(self):
        text = "Guide\n\n" + release.SOURCE_LINKS + "\n[source-example]: <../missing.txt>\n"
        with self.assertRaisesRegex(ValueError, "missing.txt"):
            release.package_markdown(text, "docs/guide.md", set(), {}, "https://example.org", "abc")
