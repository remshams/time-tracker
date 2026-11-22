"""End-to-end journeys through tt-desktop's keyboard interface."""

from __future__ import annotations

import unittest
from datetime import datetime, timedelta

from harness import Desktop, Workspace

desktop: Desktop | None = None


def setUpModule() -> None:
    global desktop
    desktop = Desktop()
    desktop.start()


def tearDownModule() -> None:
    if desktop is not None:
        desktop.stop()


class DesktopTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.workspace = Workspace()
        self.addCleanup(self.workspace.remove)
        self.data = self.workspace.data
        self.now = datetime.now().astimezone().replace(second=0, microsecond=0)

    def launch(self):
        app = self.workspace.launch()
        self.addCleanup(app.close)
        return app


class TrackingTests(DesktopTestCase):
    def test_space_starts_and_stops_the_selected_task(self) -> None:
        task = self.data.create_task("Write release notes")
        app = self.launch()
        app.wait_for("button", name="Start Write release notes")

        app.key("space")
        app.wait_for("button", contains="Stop Write release notes")
        status = self.data.tracking()
        self.assertEqual(status["state"], "running")
        self.assertEqual(status["active_worklog"]["task_id"], task)

        app.key("space")
        app.wait_for("button", name="Start Write release notes")
        self.assertEqual(self.data.tracking()["state"], "idle")
        self.assertEqual(len(self.data.worklogs(task)), 1)

    def test_starting_another_task_switches_tracking(self) -> None:
        first = self.data.create_task("Code review")
        self.data.create_task("Bug triage")
        self.data.start(first, at=self.now - timedelta(minutes=30))
        app = self.launch()
        app.wait_for("button", contains="Stop Code review")

        # Select the other task, then start it: the running worklog ends at the same instant.
        app.key("j", "space")
        app.wait_for("button", contains="Stop Bug triage")
        status = self.data.tracking()
        self.assertNotEqual(status["active_worklog"]["task_id"], first)
        self.assertIsNotNone(self.data.worklogs(first)[0]["end"])


class TaskTests(DesktopTestCase):
    def test_n_creates_a_task_and_selects_it(self) -> None:
        self.data.create_task("Existing task")
        app = self.launch()
        app.wait_for("list item", contains="Existing task")

        app.key("n")
        app.wait_for("heading", name="New task")
        app.wait_focused("Task name")
        app.type("Plan the sprint")
        app.wait_enabled("Create")
        app.key("Return")

        app.wait_gone("heading", name="New task")
        app.wait_for("heading", name="Plan the sprint")
        self.assertIn("Plan the sprint", self.data.tasks())

    def test_e_renames_the_selected_task(self) -> None:
        self.data.create_task("Old name")
        app = self.launch()
        app.wait_for("heading", name="Old name")

        app.key("e")
        app.wait_for("heading", name="Rename task")
        app.wait_focused("Task name")
        # The current name is selected, so typing replaces it.
        app.type("New name")
        app.wait_enabled("Rename")
        app.key("Return")

        app.wait_for("heading", name="New name")
        self.assertEqual(list(self.data.tasks()), ["New name"])

    def test_a_archives_after_confirming_and_unarchives_at_once(self) -> None:
        self.data.create_task("Finished project")
        app = self.launch()
        app.wait_for("heading", name="Finished project")

        app.key("a")
        app.wait_for("heading", name="Archive task")
        app.key("Return")
        app.wait_gone("heading", name="Archive task")
        app.wait_for(contains="No active tasks")
        self.assertIn("Finished project", self.data.tasks("archived"))

        app.key("Tab")
        app.wait_for("list item", contains="Finished project")
        app.key("a")
        app.wait_for(contains="No archived tasks")
        self.assertIn("Finished project", self.data.tasks("active"))

    def test_bulk_archive_archives_the_previewed_inactive_tasks(self) -> None:
        old = self.now - timedelta(days=30)
        self.data.create_task("Forgotten idea", at=old)
        recent = self.data.create_task("Current work", at=old)
        self.data.work(recent, self.now - timedelta(hours=3), 30)
        app = self.launch()
        app.wait_for("list item", contains="Forgotten idea")

        app.shifted("a")
        app.wait_for("heading", name="Archive inactive tasks")
        app.wait_for("list item", name="Forgotten idea")
        self.assertIsNone(app.find("list item", name="Current work"))
        app.key("Return")

        app.wait_for("label", contains="Archived 1")
        self.assertEqual(list(self.data.tasks("archived")), ["Forgotten idea"])
        self.assertEqual(list(self.data.tasks("active")), ["Current work"])

    def test_slash_filters_the_task_list(self) -> None:
        for name in ("Code review", "Customer call", "Bug triage"):
            self.data.create_task(name)
        app = self.launch()
        app.wait_for("list item", contains="Bug triage")

        app.key("slash")
        app.type("cus")
        app.wait_gone("list item", contains="Bug triage")
        self.assertIsNotNone(app.find("list item", contains="Customer call"))
        self.assertIsNone(app.find("list item", contains="Code review"))

        # Tab leaves the filter; it doesn't switch to the Archived tab.
        app.key("Tab")
        app.key("j")
        self.assertIsNotNone(app.find("list item", contains="Customer call"))
        self.assertIsNone(app.find(contains="No archived tasks"))

        app.key("slash")
        app.key("Escape")
        app.wait_for("list item", contains="Bug triage")


class WorklogTests(DesktopTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.task = self.data.create_task("Database migration", at=self.now - timedelta(days=3))
        self.start = (self.now - timedelta(days=1)).replace(hour=10, minute=0)
        self.worklog = self.data.work(self.task, self.start, 60)

    def test_t_edits_the_start_of_the_selected_worklog(self) -> None:
        app = self.launch()
        app.wait_for("list item", contains="10:00 to 11:00")

        app.key("l", "t")
        app.wait_for("heading", name="Edit times")
        # The start date has the focus; Tab moves to the start time and selects it.
        app.wait_focused("Start date")
        # Keys reach the app in order, so the Tab lands before the typed time.
        app.key("Tab")
        app.type("09:30")
        app.wait_enabled("Save")
        app.key("Return")

        app.wait_for("list item", contains="09:30 to 11:00")
        stored = self.data.worklogs(self.task)[0]
        start = datetime.fromisoformat(stored["start"].replace("Z", "+00:00")).astimezone()
        self.assertEqual((start.hour, start.minute), (9, 30))
        self.assertEqual(stored["id"], self.worklog)

    def test_m_moves_the_selected_worklog_to_another_task(self) -> None:
        destination = self.data.create_task("Support inbox", at=self.now - timedelta(days=3))
        app = self.launch()
        app.wait_for("list item", contains="10:00 to 11:00")

        app.key("l", "m")
        app.wait_for("heading", name="Move to task")
        app.wait_focused("Search tasks")
        app.type("supp")
        app.wait_for("list item", name="Support inbox")
        app.wait_enabled("Move")
        app.key("Return")

        app.wait_gone("heading", name="Move to task")
        self.assertEqual(self.data.worklogs(self.task), [])
        self.assertEqual([row["id"] for row in self.data.worklogs(destination)], [self.worklog])

    def test_d_deletes_the_selected_worklog_after_confirming(self) -> None:
        app = self.launch()
        app.wait_for("list item", contains="10:00 to 11:00")

        app.key("l", "d")
        app.wait_for("heading", name="Delete worklog")
        app.key("Return")

        app.wait_gone("list item", contains="10:00 to 11:00")
        self.assertEqual(self.data.worklogs(self.task), [])


if __name__ == "__main__":
    unittest.main()
