import importlib.util
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("feature_dependencies", Path(__file__).parents[1] / "feature-dependencies.py")
DEPENDENCIES = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = DEPENDENCIES
SPEC.loader.exec_module(DEPENDENCIES)

APP = Path("Sources") / "TimeTrackerSwiftUI"
CLIENT = Path("TrackerClient") / "Sources" / "TrackerClient"


def names(code: str) -> list[str]:
    return [name for name, _ in DEPENDENCIES.swift_top_level_names(DEPENDENCIES.strip_swift(code))]


class SwiftTreeCase(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def write(self, relative: Path, name: str, text: str) -> None:
        path = self.root / relative / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def target(self, name: str = "TimeTrackerSwiftUI"):
        targets = {target.name: target for target in DEPENDENCIES.extract_swift(self.root)}
        return targets[name]

    def edges(self, name: str = "TimeTrackerSwiftUI") -> set[tuple[str, str]]:
        return set(self.target(name).edges)


class StripTests(unittest.TestCase):
    def test_comments_and_nested_block_comments_are_blanked(self):
        code = DEPENDENCIES.strip_swift("a // Foo\n/* x /* Bar */ Baz */ b\n")
        self.assertNotIn("Foo", code)
        self.assertNotIn("Bar", code)
        self.assertNotIn("Baz", code)
        self.assertEqual(code.count("\n"), 2)

    def test_strings_are_blanked_in_every_form(self):
        code = DEPENDENCIES.strip_swift(
            'let a = "Foo \\"Bar\\""\nlet b = """\nBaz\n"quoted"\n"""\nlet c = #"Qux "x" \\(Quux)"#\nlet d = ##"""\nCorge\n"""##\n'
        )
        for word in ["Foo", "Bar", "Baz", "quoted", "Qux", "Quux", "Corge"]:
            self.assertNotIn(word, code)

    def test_interpolated_code_stays_code(self):
        code = DEPENDENCIES.strip_swift('let a = "Hidden \\(Used(1, (2))) Also"\nlet b = Next')
        self.assertIn("Used", code)
        self.assertNotIn("Hidden", code)
        self.assertNotIn("Also", code)
        self.assertIn("Next", code)

    def test_raw_string_interpolation_needs_the_hashes(self):
        code = DEPENDENCIES.strip_swift('let a = #"\\(Hidden) \\#(Shown)"#')
        self.assertNotIn("Hidden", code)
        self.assertIn("Shown", code)

    def test_backticks_and_single_quotes_are_not_quotes(self):
        code = DEPENDENCIES.strip_swift("let `class` = 1\nlet it = x'\nlet Used = 2")
        self.assertIn("Used", code)

    def test_unterminated_single_line_string_ends_at_the_newline(self):
        self.assertIn("Used", DEPENDENCIES.strip_swift('let a = "oops\nlet Used = 1'))


class DeclarationTests(unittest.TestCase):
    def test_kinds_modifiers_and_attributes(self):
        code = """
import SwiftUI
public final class A {}
@MainActor struct B {}
@Observable
final class C {}
@available(macOS 14, *) public enum D {}
actor E {}
protocol F {}
typealias G = Int
func h() {}
let i = 1
public var j: Int = 2
nonisolated(unsafe) var k = 3
@MainActor @Observable public final class L {}
"""
        self.assertEqual(names(code), list("ABCDEFG") + ["h", "i", "j", "k", "L"])

    def test_nested_declarations_are_not_indexed(self):
        code = "struct Outer {\n    struct Inner {}\n    func method() {}\n    let field = 1\n}\nfunc after() {}\n"
        self.assertEqual(names(code), ["Outer", "after"])

    def test_private_declarations_are_skipped(self):
        code = "private struct A {}\nfileprivate func b() {}\nprivate(set) var c = 1\nstruct D {}\n"
        # private(set) limits only the setter, so c stays visible.
        self.assertEqual(names(code), ["c", "D"])

    def test_extension_and_commented_declarations_are_not_indexed(self):
        code = "extension Foo {}\n// struct Bar {}\nlet s = \"struct Baz {}\"\n"
        self.assertEqual(names(code), ["s"])

    def test_declaration_after_a_closing_brace_is_top_level(self):
        self.assertEqual(names("struct A {\n  struct B {}\n}\nstruct C {}\n"), ["A", "C"])


class NodeTests(unittest.TestCase):
    def test_node_layout(self):
        node = DEPENDENCIES.node_of
        self.assertEqual(node(Path("Features/Tracking/View.swift")), "Features/Tracking")
        self.assertEqual(node(Path("Features/Tracking/Deep/More/View.swift")), "Features/Tracking")
        self.assertEqual(node(Path("Infrastructure/System/Clock.swift")), "Infrastructure/System")
        self.assertEqual(node(Path("Shared/Tokens/Colors.swift")), "Shared")
        self.assertEqual(node(Path("Contracts/Models.swift")), "Contracts")
        self.assertEqual(node(Path("App/Main.swift")), "App")
        self.assertEqual(node(Path("Main.swift")), "App")


class ExtractionTests(SwiftTreeCase):
    def test_use_of_another_folders_declaration_is_an_edge(self):
        self.write(APP, "Features/A/A.swift", "struct AView { let b = BStore() }\n")
        self.write(APP, "Features/B/B.swift", "final class BStore {}\n")
        self.assertEqual(self.edges(), {("Features/A", "Features/B")})
        location = self.target().edges[("Features/A", "Features/B")][0]
        self.assertEqual(location.line, 1)

    def test_use_inside_the_same_folder_is_ignored(self):
        self.write(APP, "Features/A/A.swift", "struct AView { let b = AStore() }\n")
        self.write(APP, "Features/A/Deeper/Store.swift", "final class AStore {}\n")
        self.assertEqual(self.edges(), set())

    def test_deeper_folders_fold_into_the_feature_node(self):
        self.write(APP, "Features/A/Sub/A.swift", "struct AView { let b = BStore() }\n")
        self.write(APP, "Features/B/Sub/Deep/B.swift", "final class BStore {}\n")
        self.assertEqual(self.edges(), {("Features/A", "Features/B")})

    def test_infrastructure_folders_are_separate_nodes(self):
        self.write(APP, "App/App.swift", "let x = Clock()\n")
        self.write(APP, "Infrastructure/System/Clock.swift", "struct Clock {}\n")
        self.write(APP, "Infrastructure/Preferences/Prefs.swift", "struct Prefs {}\n")
        self.assertEqual(self.edges(), {("App", "Infrastructure/System")})
        self.assertIn("Infrastructure/Preferences", self.target().nodes)

    def test_extension_counts_as_a_use(self):
        self.write(APP, "Features/A/A.swift", "extension BStore { var x: Int { 1 } }\n")
        self.write(APP, "Features/B/B.swift", "final class BStore {}\n")
        self.assertEqual(self.edges(), {("Features/A", "Features/B")})

    def test_top_level_functions_and_constants_count(self):
        self.write(APP, "Features/A/A.swift", "let a = format(limit)\n")
        self.write(APP, "Shared/S.swift", "func format(_ x: Int) -> String { \"\" }\nlet limit = 3\n")
        self.assertEqual(self.edges(), {("Features/A", "Shared")})

    def test_a_lowercase_member_after_a_dot_is_not_a_use(self):
        self.write(APP, "Features/A/A.swift", "let a = value.format\n")
        self.write(APP, "Shared/S.swift", "func format() {}\n")
        self.assertEqual(self.edges(), set())

    def test_hidden_references_do_not_count(self):
        self.write(
            APP,
            "Features/A/A.swift",
            '// BStore\n/* BStore */\nlet a = "BStore"\nlet b = """\nBStore\n"""\nlet c = #"BStore"#\nlet d = `BStoreX`\n',
        )
        self.write(APP, "Features/B/B.swift", "final class BStore {}\n")
        self.assertEqual(self.edges(), set())

    def test_reference_in_interpolation_counts(self):
        self.write(APP, "Features/A/A.swift", 'let a = "total \\(BStore.count)"\n')
        self.write(APP, "Features/B/B.swift", "enum BStore { static let count = 1 }\n")
        self.assertEqual(self.edges(), {("Features/A", "Features/B")})

    def test_private_declarations_are_not_targets_of_edges(self):
        self.write(APP, "Features/A/A.swift", "let a = Hidden()\n")
        self.write(APP, "Features/B/B.swift", "private struct Hidden {}\n")
        self.assertEqual(self.edges(), set())

    def test_nested_declarations_are_not_targets_of_edges(self):
        self.write(APP, "Features/A/A.swift", "let a = Inner()\n")
        self.write(APP, "Features/B/B.swift", "struct Outer { struct Inner {} }\n")
        self.assertEqual(self.edges(), set())

    def test_names_are_matched_as_whole_words(self):
        self.write(APP, "Features/A/A.swift", "let a = BStoreExtra()\n")
        self.write(APP, "Features/B/B.swift", "final class BStore {}\n")
        self.assertEqual(self.edges(), set())

    def test_duplicate_names_across_folders_are_reported(self):
        self.write(APP, "Features/A/A.swift", "struct Same {}\n")
        self.write(APP, "Features/B/B.swift", "struct Same {}\n")
        self.write(APP, "Features/C/C.swift", "let c = Same()\n")
        target = self.target()
        self.assertEqual(len(target.problems), 1)
        self.assertIn("declares Same in more than one folder", target.problems[0])
        self.assertIn("Features/A", target.problems[0])
        self.assertIn("Features/B", target.problems[0])
        self.assertEqual(set(target.edges), set())

    def test_a_file_in_the_target_root_belongs_to_app(self):
        self.write(APP, "Main.swift", "let a = BStore()\n")
        self.write(APP, "Features/B/B.swift", "final class BStore {}\n")
        self.assertEqual(self.edges(), {("App", "Features/B")})

    def test_only_swift_files_count(self):
        self.write(APP, "Features/A/A.swift", "let a = 1\n")
        self.write(APP, "Features/B/B.h", "struct BStore {}\n")
        self.assertEqual(self.target().nodes, {"Features/A"})

    def test_targets_are_checked_separately(self):
        self.write(APP, "Features/A/A.swift", "let a = BStore()\n")
        self.write(APP, "Features/B/B.swift", "let b = 1\n")
        self.write(CLIENT, "Features/B/B.swift", "final class BStore {}\n")
        self.write(CLIENT, "Features/A/A.swift", "let a = 1\n")
        self.assertEqual(self.edges("TimeTrackerSwiftUI"), set())
        self.assertEqual(self.edges("TrackerClient"), set())

    def test_both_targets_are_returned_with_their_names(self):
        self.write(APP, "App/A.swift", "let a = 1\n")
        self.write(CLIENT, "App/A.swift", "let a = 1\n")
        self.assertEqual([target.name for target in DEPENDENCIES.extract_swift(self.root)], ["TimeTrackerSwiftUI", "TrackerClient"])


class MainTests(SwiftTreeCase):
    def run_main(self, config: str) -> tuple[int, str]:
        config_path = self.root / "feature-dependencies.toml"
        config_path.write_text(config)
        error = StringIO()
        with redirect_stdout(StringIO()), redirect_stderr(error):
            code = DEPENDENCIES.main(["--stack", "swift", "--source", str(self.root), "--config", str(config_path)])
        return code, error.getvalue()

    def sources(self):
        self.write(APP, "Features/A/A.swift", "let a = BStore()\n")
        self.write(APP, "Features/B/B.swift", "final class BStore {}\n")
        self.write(CLIENT, "Contracts/C.swift", "struct Model {}\n")
        self.write(CLIENT, "Features/A/A.swift", "let a = Model()\n")

    CONFIG = """
[targets.TimeTrackerSwiftUI]
implicit = []
[targets.TimeTrackerSwiftUI.nodes."Features/A"]
uses = [{uses}]
[targets.TimeTrackerSwiftUI.nodes."Features/B"]
uses = []
[targets.TrackerClient]
implicit = ["Contracts"]
[targets.TrackerClient.nodes."Contracts"]
uses = []
[targets.TrackerClient.nodes."Features/A"]
uses = []
"""

    def test_missing_edge_fails(self):
        self.sources()
        code, error = self.run_main(self.CONFIG.format(uses=""))
        self.assertEqual(code, 1)
        self.assertIn("Features/A uses Features/B, which", error)
        self.assertIn("Features/A/A.swift:1", error)

    def test_matching_config_passes(self):
        self.sources()
        code, error = self.run_main(self.CONFIG.format(uses='"Features/B"'))
        self.assertEqual((code, error), (0, ""))

    def test_cycle_fails(self):
        self.write(APP, "Features/A/A.swift", "let a = BStore()\nstruct AStore {}\n")
        self.write(APP, "Features/B/B.swift", "final class BStore { let a = AStore() }\n")
        config = """
[targets.TimeTrackerSwiftUI]
implicit = []
[targets.TimeTrackerSwiftUI.nodes."Features/A"]
uses = ["Features/B"]
[targets.TimeTrackerSwiftUI.nodes."Features/B"]
uses = ["Features/A"]
"""
        code, error = self.run_main(config)
        self.assertEqual(code, 1)
        self.assertIn("cycle: Features/A -> Features/B -> Features/A", error)


class RepositoryTests(unittest.TestCase):
    def test_the_repository_matches_its_config(self):
        output, error = StringIO(), StringIO()
        with redirect_stdout(output), redirect_stderr(error):
            code = DEPENDENCIES.main(["--stack", "swift"])
        self.assertEqual((code, error.getvalue()), (0, ""))


if __name__ == "__main__":
    unittest.main()
