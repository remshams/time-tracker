import importlib.util
import io
import sys
import tempfile
import textwrap
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path


SPEC = importlib.util.spec_from_file_location("feature_dependencies", Path(__file__).parents[1] / "feature-dependencies.py")
DEPENDENCIES = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = DEPENDENCIES
SPEC.loader.exec_module(DEPENDENCIES)

CONTROLLER_HEADER = """
#pragma once
class CatalogStore;
class AppController : public QObject {
private:
    CatalogStore *catalog_;
    Theme *theme_;
};
"""
CONTROLLER = """
void AppController::expose(QQmlContext *context) {
    context->setContextProperty(QStringLiteral("catalog"), catalog_);
    context->setContextProperty(QStringLiteral("theme"), theme_);
}
"""


def write(root, files):
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text))


def target(edges, nodes=None, name=None):
    result = DEPENDENCIES.Target(name)
    for source, used in edges:
        result.add(source, used, DEPENDENCIES.Location(Path(f"{source}.cpp"), 1))
    result.nodes = set(nodes) if nodes is not None else {node for edge in edges for node in edge}
    return result


def config(text):
    return DEPENDENCIES.parse_config(textwrap.dedent(text))


def check(found, text):
    return DEPENDENCIES.check_all([found], config(text), Path("deps.toml"))


class StripTests(unittest.TestCase):
    def test_comments_and_string_contents_are_blanked_but_lines_stay(self):
        text = 'a // catalog.x\n/* catalog.y\n */ b "catalog.z" \'catalog.w\' c'
        stripped = DEPENDENCIES.strip_comments_and_strings(text)
        self.assertNotIn("catalog", stripped)
        self.assertEqual(stripped.count("\n"), text.count("\n"))
        self.assertEqual(len(stripped), len(text))
        self.assertIn("a", stripped)
        self.assertIn("b", stripped)
        self.assertTrue(stripped.rstrip().endswith("c"))

    def test_escaped_quote_stays_inside_the_string(self):
        stripped = DEPENDENCIES.strip_comments_and_strings('"say \\"catalog.x\\"" catalog.y')
        self.assertEqual(stripped.count("catalog"), 1)


class OmarchyExtractorTests(unittest.TestCase):
    def extract(self, files):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root, files)
            [found] = DEPENDENCIES.extract_omarchy(root)
            return found, root

    def edges(self, files):
        found, _ = self.extract(files)
        return set(found.edges)

    def test_nodes_follow_the_top_level_and_feature_folders(self):
        found, _ = self.extract({
            "main.cpp": "",
            "App/Main.qml": "",
            "Shared/Theme.h": "",
            "Features/Catalog/Store.h": "",
            "Features/Catalog/Views/List.qml": "",
        })
        self.assertEqual(found.nodes, {"App", "Shared", "Features/Catalog"})

    def test_include_prefers_the_file_folder_like_the_compiler(self):
        edges = self.edges({
            "Features/Catalog/Store.cpp": '#include "Shared/Format.h"',
            "Features/Catalog/Shared/Format.h": "",
            "Shared/Format.h": "",
        })
        self.assertEqual(edges, set())

    def test_includes_resolve_against_src_and_the_file_folder(self):
        edges = self.extract({
            "main.cpp": '#include "App/AppController.h"\n#include <QObject>\n#include "Shared/Theme.h"',
            "App/AppController.h": "",
            "Shared/Theme.h": "",
            "Features/Catalog/Store.cpp": '#include "Store.h"\n#include "Features/Tracking/Tracker.h"\n#include "bridge.h"',
            "Features/Catalog/Store.h": "",
            "Features/Tracking/Tracker.h": "",
        })[0].edges
        self.assertEqual(set(edges), {("App", "Shared"), ("Features/Catalog", "Features/Tracking")})
        self.assertEqual(edges[("Features/Catalog", "Features/Tracking")][0].line, 2)

    def test_example_line_is_the_include_not_the_blank_line_above(self):
        found, _ = self.extract({
            "Features/Catalog/Store.h": '#pragma once\n\n\n#include "Shared/Theme.h"\n',
            "Features/Catalog/View.qml": 'import QtQuick\n\nimport "../../Shared"\n',
            "Shared/Theme.h": "",
        })
        lines = sorted(location.line for location in found.edges[("Features/Catalog", "Shared")])
        self.assertEqual(lines, [3, 4])

    def test_commented_include_does_not_count(self):
        edges = self.edges({
            "Features/Catalog/Store.cpp": '// #include "Shared/Theme.h"\n/* #include "Shared/Format.h" */',
            "Shared/Theme.h": "",
            "Shared/Format.h": "",
        })
        self.assertEqual(edges, set())

    def test_qml_imports_resolve_relative_to_the_file(self):
        edges = self.edges({
            "App/Main.qml": 'import QtQuick\nimport "../Features/Catalog"\nimport "../Shared"',
            "Features/Catalog/Dialog.qml": 'import "../../Shared"\nimport "../../Missing"',
            "Shared/Txt.qml": "",
        })
        self.assertEqual(edges, {("App", "Features/Catalog"), ("App", "Shared"), ("Features/Catalog", "Shared")})

    def test_context_properties_resolve_through_the_controller_to_the_type_folder(self):
        edges = self.edges({
            "App/AppController.h": CONTROLLER_HEADER,
            "App/AppController.cpp": CONTROLLER,
            "Features/Catalog/CatalogStore.h": "class CatalogStore : public QObject {};",
            "Shared/Theme.h": "class Theme : public QObject {};",
            "Features/History/Detail.qml": "\n".join([
                "Item {",
                "    color: theme.accent",
                "    text: catalog.selectedName",
                "    // catalog.ignored",
                '    title: "catalog.ignored"',
                "    property var other: model.catalog.name",
                "}",
            ]),
        })
        self.assertEqual(edges, {("Features/History", "Features/Catalog"), ("Features/History", "Shared")})

    def test_context_property_uses_without_a_dot_count(self):
        edges = self.edges({
            "App/AppController.h": CONTROLLER_HEADER,
            "App/AppController.cpp": CONTROLLER,
            "Features/Catalog/CatalogStore.h": "class CatalogStore : public QObject {};",
            "Shared/Theme.h": "class Theme : public QObject {};",
            "Features/Move/Dialog.qml": "Connections {\n    target: catalog\n}",
            "Features/Rename/Dialog.qml": "Item { Component.onCompleted: load(catalog) }",
            # A line that starts with the name is a use when the colon belongs to a ternary below it.
            "Features/Delete/Dialog.qml": "Item {\n    property var store: shown ?\n        catalog\n        : null\n}",
            "Features/Archive/Dialog.qml": "Item {\n    property var store: shown ?\n        catalog : null\n}",
        })
        self.assertEqual(edges, {
            ("Features/Move", "Features/Catalog"),
            ("Features/Rename", "Features/Catalog"),
            ("Features/Delete", "Features/Catalog"),
            ("Features/Archive", "Features/Catalog"),
        })

    def test_properties_named_like_a_context_property_are_not_uses(self):
        edges = self.edges({
            "App/AppController.h": CONTROLLER_HEADER,
            "App/AppController.cpp": CONTROLLER,
            "Features/Catalog/CatalogStore.h": "class CatalogStore : public QObject {};",
            "Shared/Theme.h": "class Theme : public QObject {};",
            "Features/Move/Dialog.qml": "\n".join([
                "Item {",
                "    property var catalog",
                "    required property string catalog",
                "    catalog: null",
                "    readonly property bool shown: model.catalog",
                "}",
            ]),
        })
        self.assertEqual(edges, set())

    def test_commented_out_context_property_is_not_a_property(self):
        edges = self.edges({
            "App/AppController.h": CONTROLLER_HEADER,
            "App/AppController.cpp": CONTROLLER.replace("    context->setContextProperty(QStringLiteral(\"catalog\")", "    // context->setContextProperty(QStringLiteral(\"catalog\")"),
            "Features/Catalog/CatalogStore.h": "class CatalogStore : public QObject {};",
            "Shared/Theme.h": "class Theme : public QObject {};",
            "Features/Move/Dialog.qml": "Text { text: catalog.name }",
        })
        self.assertEqual(edges, set())

    def test_unreadable_context_property_call_is_a_problem(self):
        found, _ = self.extract({
            "App/AppController.h": CONTROLLER_HEADER,
            "App/AppController.cpp": CONTROLLER + 'void f(QQmlContext *c) {\n    c->setContextProperty("other", other_);\n    // c->setContextProperty("old", old_);\n}\n',
            "Features/Catalog/CatalogStore.h": "class CatalogStore : public QObject {};",
            "Shared/Theme.h": "class Theme : public QObject {};",
        })
        self.assertEqual(len(found.problems), 1)
        self.assertIn("AppController.cpp:7: can't read this context property", found.problems[0])

    def test_unresolved_context_property_is_a_problem(self):
        found, _ = self.extract({
            "App/AppController.h": "class AppController { Missing *catalog_; Theme *theme_; };",
            "App/AppController.cpp": CONTROLLER,
            "Shared/Theme.h": "class Theme {};",
        })
        self.assertEqual(len(found.problems), 1)
        self.assertIn("'catalog'", found.problems[0])
        self.assertIn("AppController.cpp:3", found.problems[0])

    def test_no_controller_means_no_context_properties(self):
        found, _ = self.extract({"Features/Catalog/List.qml": "text: catalog.name"})
        self.assertEqual((set(found.edges), found.problems), (set(), []))


class CheckTests(unittest.TestCase):
    CONFIG = """
        implicit = ["Shared"]

        [nodes."App"]
        uses = ["Features/A", "Features/B"]

        [nodes."Shared"]
        uses = []

        [nodes."Features/A"]
        # A note the check ignores.
        uses = ["Features/B"]

        [nodes."Features/B"]
        uses = []
    """
    EDGES = [("App", "Features/A"), ("App", "Features/B"), ("Features/A", "Features/B"), ("Features/A", "Shared")]

    def test_matching_config_passes(self):
        self.assertEqual(check(target(self.EDGES), self.CONFIG), [])

    def test_unlisted_edge_fails_with_the_line_to_add_and_examples(self):
        found = target(self.EDGES + [("Features/B", "Features/A")])
        problems = check(found, self.CONFIG.replace('[nodes."Features/A"]', '[nodes."Features/C"]\nuses = []\n\n[nodes."Features/A"]'))
        missing = [problem for problem in problems if "doesn't list" in problem]
        self.assertEqual(len(missing), 1)
        self.assertIn('Add "Features/A" to uses in [nodes."Features/B"]', missing[0])
        self.assertIn("Features/B.cpp:1", missing[0])

    def test_examples_are_capped(self):
        found = target(self.EDGES)
        for line in range(10):
            found.add("Features/B", "Features/C", DEPENDENCIES.Location(Path("B.swift"), line))
        found.nodes.add("Features/C")
        text = self.CONFIG + '\n[nodes."Features/C"]\nuses = []\n'
        [problem] = check(found, text)
        self.assertEqual(problem.count("B.swift:"), DEPENDENCIES.EXAMPLES)

    def test_stale_edge_fails(self):
        found = target([edge for edge in self.EDGES if edge != ("Features/A", "Features/B")])
        self.assertEqual(
            check(found, self.CONFIG),
            ['[nodes."Features/A"] lists "Features/B", but no file in Features/A uses it. Remove it.'],
        )

    def test_listing_an_implicit_node_fails(self):
        text = self.CONFIG.replace('uses = ["Features/B"]', 'uses = ["Features/B", "Shared"]')
        self.assertEqual(check(target(self.EDGES), text), ['[nodes."Features/A"] lists "Shared", which is implicit. Remove it.'])

    def test_missing_and_unknown_sections_fail(self):
        found = target(self.EDGES + [("Features/New", "Features/A"), ("Features/New", "Shared")])
        text = self.CONFIG + '\n[nodes."Features/Gone"]\nuses = []\n'
        problems = check(found, text)
        self.assertEqual(len(problems), 2)
        # The section to add already lists the new folder's edges, so one fix is enough.
        self.assertIn('no section for the folder Features/New. Add one:\n    [nodes."Features/New"]\n    uses = ["Features/A"]', problems[0])
        self.assertIn('has [nodes."Features/Gone"], but no folder has that name', problems[1])

    def test_unknown_implicit_node_fails(self):
        text = self.CONFIG.replace('implicit = ["Shared"]', 'implicit = ["Shared", "Contracts"]')
        self.assertEqual(
            check(target(self.EDGES), text),
            ["deps.toml makes 'Contracts' implicit, but no folder has that name. Remove it from implicit."],
        )

    def test_cycle_fails_even_when_every_edge_is_listed(self):
        found = target(self.EDGES + [("Features/B", "App")])
        text = self.CONFIG.replace('[nodes."Features/B"]\n        uses = []', '[nodes."Features/B"]\n        uses = ["App"]')
        [problem] = check(found, text)
        self.assertIn("The folders form a cycle: App -> Features/B -> App. Remove one of these dependencies:", problem)
        self.assertIn("    Features/B -> App: Features/B.cpp:1", problem)

    def test_dependency_onto_a_feature_that_does_not_use_the_implicit_node_is_a_cycle(self):
        # Features/B never uses Shared, but it may at any time, so Shared -> Features/B is a cycle.
        found = target(self.EDGES + [("Shared", "Features/B")])
        text = self.CONFIG.replace('[nodes."Shared"]\n        uses = []', '[nodes."Shared"]\n        uses = ["Features/B"]')
        [problem] = check(found, text)
        self.assertIn("The folders form a cycle: Features/B -> Shared -> Features/B.", problem)
        self.assertIn("    Features/B -> Shared: Shared is implicit", problem)
        self.assertIn("    Shared -> Features/B: Shared.cpp:1", problem)

    def test_implicit_nodes_do_not_form_a_cycle_with_each_other(self):
        found = target([("Shared", "Contracts")], nodes={"App", "Shared", "Contracts"})
        text = 'implicit = ["Shared", "Contracts"]\n[nodes."App"]\nuses = []\n[nodes."Shared"]\nuses = []\n[nodes."Contracts"]\nuses = []'
        self.assertEqual(check(found, text), [])

    def test_cycle_through_an_implicit_node_fails(self):
        found = target(self.EDGES + [("Shared", "Features/A")])
        text = self.CONFIG.replace('[nodes."Shared"]\n        uses = []', '[nodes."Shared"]\n        uses = ["Features/A"]')
        [problem] = check(found, text)
        self.assertIn("Features/A -> Shared -> Features/A", problem)

    def test_extractor_problems_are_reported(self):
        found = target(self.EDGES)
        found.problems.append("unresolved")
        self.assertEqual(check(found, self.CONFIG), ["unresolved"])


class TargetTests(unittest.TestCase):
    def test_targets_are_checked_against_their_own_section(self):
        text = """
            [targets.App.nodes."Features/A"]
            uses = ["Features/B"]
            [targets.App.nodes."Features/B"]
            uses = []
            [targets.Package.nodes."Features/A"]
            uses = []
        """
        targets = [target([("Features/A", "Features/B")], name="App"), target([], nodes={"Features/A"}, name="Package")]
        self.assertEqual(DEPENDENCIES.check_all(targets, config(text), Path("deps.toml")), [])
        targets[1].add("Features/A", "Features/B", DEPENDENCIES.Location(Path("a.swift"), 1))
        targets[1].nodes.add("Features/B")
        problems = DEPENDENCIES.check_all(targets, config(text), Path("deps.toml"))
        self.assertIn('[targets.Package.nodes."Features/B"]\n    uses = []', problems[0])

    def test_missing_and_unknown_targets_fail(self):
        problems = DEPENDENCIES.check_all(
            [target([], nodes=set(), name="App")], config("[targets.Old]\nimplicit = []"), Path("deps.toml")
        )
        self.assertEqual(
            problems,
            [
                "deps.toml has a section for target 'Old', which doesn't exist. Remove it.",
                "deps.toml has no config for target 'App'. Add it under [targets.App].",
            ],
        )


class CycleTests(unittest.TestCase):
    def test_acyclic_graph_has_no_cycles(self):
        self.assertEqual(DEPENDENCIES.cycles({("A", "B"), ("B", "C"), ("A", "C")}), [])

    def test_one_shortest_cycle_per_component(self):
        edges = {("A", "B"), ("B", "C"), ("C", "A"), ("B", "A"), ("D", "E"), ("E", "D")}
        self.assertEqual(DEPENDENCIES.cycles(edges), [["A", "B", "A"], ["D", "E", "D"]])


class MainTests(unittest.TestCase):
    def run_main(self, files, config_text):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "src", files)
            (root / "deps.toml").write_text(textwrap.dedent(config_text))
            output, errors = io.StringIO(), io.StringIO()
            with redirect_stdout(output), redirect_stderr(errors):
                code = DEPENDENCIES.main(["--stack", "omarchy", "--source", str(root / "src"), "--config", str(root / "deps.toml")])
            return code, output.getvalue(), errors.getvalue()

    FILES = {"App/Main.qml": 'import "../Features/A"', "Features/A/View.qml": ""}

    def test_match_exits_zero(self):
        code, output, _ = self.run_main(self.FILES, '[nodes."App"]\nuses = ["Features/A"]\n[nodes."Features/A"]\nuses = []')
        self.assertEqual(code, 0)
        self.assertIn("Feature dependencies match", output)

    def test_mismatch_exits_one_with_a_summary(self):
        code, _, errors = self.run_main(self.FILES, '[nodes."App"]\nuses = []\n[nodes."Features/A"]\nuses = []')
        self.assertEqual(code, 1)
        self.assertIn('Add "Features/A" to uses in [nodes."App"]', errors)
        self.assertIn("1 feature dependency problem(s)", errors)

    def test_unreadable_config_exits_one(self):
        code, _, errors = self.run_main(self.FILES, "nodes = [")
        self.assertEqual(code, 1)
        self.assertIn("Can't read", errors)

    def test_repository_omarchy_config_matches(self):
        output, errors = io.StringIO(), io.StringIO()
        with redirect_stdout(output), redirect_stderr(errors):
            code = DEPENDENCIES.main(["--stack", "omarchy"])
        self.assertEqual(code, 0, errors.getvalue())


if __name__ == "__main__":
    unittest.main()
