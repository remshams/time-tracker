#include <QSignalSpy>
#include <QTest>

#include "Features/WorklogMove/WorklogMoveStore.h"
#include "TestRegistry.h"
#include "WorklogFixture.h"

namespace {

const QString startText = QStringLiteral("2026-11-20T08:00:30.250Z");
const QString endText = QStringLiteral("2026-11-20T09:30:45.500Z");

// The core's answer: [{id, name}].
BridgeReply candidates(const QList<QPair<QString, QString>> &tasks) {
    QJsonArray rows;
    for (const auto &[id, name] : tasks) {
        rows.append(QJsonObject{{QStringLiteral("id"), id}, {QStringLiteral("name"), name}});
    }
    return BridgeReply::success(rows);
}

BridgeReply betaAndGamma() {
    return candidates({{"task-b", "Beta"}, {"task-c", "Gamma"}});
}

QString idAt(const WorklogMoveStore &store, int index) {
    return store.candidates().at(index).toMap().value(QStringLiteral("id")).toString();
}

// A fixture with the two worklogs the tests move: `w1` completed and `w2` running.
struct MoveFixture : WorklogFixture {
    WorklogMoveStore store{&gateway, &history};

    MoveFixture() {
        showHistory({WorklogFixture::worklog("w1", startText, endText), WorklogFixture::worklog("w2", startText)});
    }

    // The `n`th search the store sent, counted from zero. The history call comes first in `calls`.
    FakeTrackerGateway::Call &search(int n) {
        int seen = 0;
        for (FakeTrackerGateway::Call &call : gateway.calls) {
            if (call.method == QStringLiteral("moveCandidates") && seen++ == n) {
                return call;
            }
        }
        qFatal("No search number %d", n);
    }

    // Opens `w1` and answers the first search.
    void openWithResults(const QString &worklogId = QStringLiteral("w1")) {
        QVERIFY(store.begin(worklogId));
        QVERIFY(gateway.respond("moveCandidates", betaAndGamma()));
    }
};

} // namespace

class WorklogMoveTests : public QObject {
    Q_OBJECT

private slots:
    void beginCapturesTheWorklogAndSearchesForAnEmptyQuery() {
        MoveFixture f;
        QSignalSpy begun(&f.store, &WorklogMoveStore::begun);

        QVERIFY(f.store.begin("w1"));

        QCOMPARE(begun.count(), 1);
        QCOMPARE(f.store.sourceTaskId(), QStringLiteral("task-a"));
        QVERIFY(!f.store.running());
        QCOMPARE(f.store.start(), QDateTime::fromString(startText, Qt::ISODateWithMs));
        QCOMPARE(f.store.end(), QDateTime::fromString(endText, Qt::ISODateWithMs));
        QCOMPARE(f.gateway.count("moveCandidates"), 1);
        QCOMPARE(f.gateway.lastArgs("moveCandidates"), (QStringList{"task-a", QString()}));
        QVERIFY(f.store.searching());
        QVERIFY(!f.store.fresh());
        QVERIFY(!f.store.canMove());

        f.gateway.respond("moveCandidates", betaAndGamma());

        QCOMPARE(f.store.candidates().size(), 2);
        QCOMPARE(idAt(f.store, 0), QStringLiteral("task-b"));
        QCOMPARE(f.store.candidates().at(0).toMap().value("name").toString(), QStringLiteral("Beta"));
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-b"));
        QCOMPARE(f.store.currentIndex(), 0);
        QVERIFY(!f.store.searching());
        QVERIFY(f.store.fresh());
        QVERIFY(f.store.canMove());
    }

    void beginRefusesAWorklogThatIsNotShown() {
        MoveFixture f;

        QVERIFY(!f.store.begin("missing"));

        QCOMPARE(f.gateway.count("moveCandidates"), 0);
        QVERIFY(!f.store.canMove());
        QVERIFY(f.store.sourceTaskId().isEmpty());
        QVERIFY(!f.store.running());
        QVERIFY(!f.store.start().isValid());
        QVERIFY(!f.store.end().isValid());
    }

    void aRunningWorklogHasNoEnd() {
        MoveFixture f;

        QVERIFY(f.store.begin("w2"));

        QVERIFY(f.store.running());
        QVERIFY(!f.store.end().isValid());
    }

    void eachQueryChangeSendsOneSearch() {
        MoveFixture f;
        f.openWithResults();
        QSignalSpy queryChanged(&f.store, &WorklogMoveStore::queryChanged);

        f.store.setQuery("be");
        f.store.setQuery("be"); // The same text sends nothing.
        f.store.setQuery("bet");

        QCOMPARE(queryChanged.count(), 2);
        QCOMPARE(f.store.query(), QStringLiteral("bet"));
        QCOMPARE(f.gateway.count("moveCandidates"), 3);
        QCOMPARE(f.search(1).args, (QStringList{"task-a", "be"}));
        QCOMPARE(f.search(2).args, (QStringList{"task-a", "bet"}));
    }

    void answersForAnOlderQueryAreDropped() {
        MoveFixture f;
        QVERIFY(f.store.begin("w1"));
        f.store.setQuery("b");
        f.store.setQuery("be");
        QCOMPARE(f.gateway.count("moveCandidates"), 3);

        // The newest answer arrives first, then the two older ones.
        f.search(2).done(candidates({{"task-b", "Beta"}}));
        QCOMPARE(f.store.candidates().size(), 1);
        f.search(1).done(betaAndGamma());
        f.search(0).done(candidates({}));

        QCOMPARE(f.store.candidates().size(), 1);
        QCOMPARE(idAt(f.store, 0), QStringLiteral("task-b"));
        QVERIFY(f.store.fresh());
        QVERIFY(!f.store.searching());
    }

    void aFailureForAnOlderQueryDoesNotShowAnError() {
        MoveFixture f;
        QVERIFY(f.store.begin("w1"));
        f.store.setQuery("be");

        f.search(0).done(BridgeReply::failure("offline"));

        QVERIFY(f.store.error().isEmpty());
        QVERIFY(f.store.searching());
    }

    void priorResultsStayWhileASearchRunsButTheMoveWaits() {
        MoveFixture f;
        f.openWithResults();
        QSignalSpy canMoveChanged(&f.store, &WorklogMoveStore::canMoveChanged);

        f.store.setQuery("g");

        QCOMPARE(f.store.candidates().size(), 2);
        QVERIFY(f.store.searching());
        QVERIFY(!f.store.fresh());
        QVERIFY(!f.store.canMove());
        QCOMPARE(canMoveChanged.count(), 1);
        f.store.move();
        QCOMPARE(f.gateway.count("moveWorklog"), 0);
        // The user can still pick from the old results while the search runs.
        f.store.moveSelection(1);
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-c"));

        f.gateway.respond("moveCandidates", candidates({{"task-c", "Gamma"}}));

        QVERIFY(f.store.fresh());
        QVERIFY(f.store.canMove());
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-c"));
        QCOMPARE(canMoveChanged.count(), 2);
    }

    void theChosenTaskStaysWhileListedAndOtherwiseTheBestMatchIsChosen() {
        MoveFixture f;
        f.openWithResults();
        f.store.moveSelection(1);
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-c"));

        f.store.setQuery("a");
        f.gateway.respond("moveCandidates", candidates({{"task-b", "Beta"}, {"task-c", "Gamma"}, {"task-d", "Delta"}}));
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-c"));
        QCOMPARE(f.store.currentIndex(), 1);

        f.store.setQuery("d");
        f.gateway.respond("moveCandidates", candidates({{"task-d", "Delta"}}));
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-d"));

        f.store.setQuery("zzz");
        f.gateway.respond("moveCandidates", candidates({}));
        QVERIFY(f.store.candidates().isEmpty());
        QVERIFY(f.store.selectedTaskId().isEmpty());
        QCOMPARE(f.store.currentIndex(), -1);
        QVERIFY(f.store.fresh());
        QVERIFY(!f.store.canMove());
    }

    void selectionStopsAtTheEnds() {
        MoveFixture f;
        f.openWithResults();
        QSignalSpy selection(&f.store, &WorklogMoveStore::selectionChanged);

        f.store.moveSelection(-1);
        QCOMPARE(f.store.currentIndex(), 0);
        QCOMPARE(selection.count(), 0);
        f.store.moveSelection(1);
        f.store.moveSelection(1);
        QCOMPARE(f.store.currentIndex(), 1);
        QCOMPARE(selection.count(), 1);
        f.store.select(0);
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-b"));
        f.store.select(99);
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-c"));
        f.store.select(-5);
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-b"));
    }

    void selectionDoesNothingWithoutCandidates() {
        MoveFixture f;
        QVERIFY(f.store.begin("w1"));
        QSignalSpy selection(&f.store, &WorklogMoveStore::selectionChanged);

        f.store.moveSelection(1);
        f.store.select(0);

        QCOMPARE(selection.count(), 0);
        QCOMPARE(f.store.currentIndex(), -1);
    }

    void moveSendsTheOriginalTaskAndTimestampsAndSucceeds() {
        MoveFixture f;
        f.openWithResults();
        f.store.moveSelection(1);
        QSignalSpy moved(&f.store, &WorklogMoveStore::moved);
        QSignalSpy refresh(&f.store, &WorklogMoveStore::refreshRequested);
        QSignalSpy pendingChanged(&f.store, &WorklogMoveStore::pendingChanged);

        f.store.move();

        QCOMPARE(f.gateway.lastArgs("moveWorklog"), (QStringList{"w1", "task-a", startText, endText, "task-c"}));
        QVERIFY(f.store.pending());
        QVERIFY(!f.store.canMove());
        QCOMPARE(pendingChanged.count(), 1);
        f.store.move(); // A second move while one is open sends nothing.
        QCOMPARE(f.gateway.count("moveWorklog"), 1);
        QCOMPARE(moved.count(), 0);
        QCOMPARE(refresh.count(), 0);

        f.gateway.respond("moveWorklog", writeSucceeded());

        QVERIFY(!f.store.pending());
        QCOMPARE(moved.count(), 1);
        QCOMPARE(moved.first().first().toString(), QStringLiteral("task-c"));
        QCOMPARE(refresh.count(), 1);
        QVERIFY(f.store.error().isEmpty());
    }

    void aRunningWorklogMovesWithAnEmptyExpectedEnd() {
        MoveFixture f;
        f.openWithResults("w2");

        f.store.move();

        QCOMPARE(f.gateway.lastArgs("moveWorklog"), (QStringList{"w2", "task-a", startText, QString(), "task-b"}));
    }

    void aRefusedMoveKeepsTheSearchAndTheSelection() {
        MoveFixture f;
        f.openWithResults();
        f.store.setQuery("ga");
        f.gateway.respond("moveCandidates", candidates({{"task-c", "Gamma"}}));
        QSignalSpy moved(&f.store, &WorklogMoveStore::moved);
        QSignalSpy refresh(&f.store, &WorklogMoveStore::refreshRequested);

        f.store.move();
        f.gateway.respond("moveWorklog", BridgeReply::failure("The worklog overlaps another one"));

        QCOMPARE(f.store.error(), QStringLiteral("The worklog overlaps another one"));
        QVERIFY(!f.store.pending());
        QCOMPARE(f.store.query(), QStringLiteral("ga"));
        QCOMPARE(f.store.selectedTaskId(), QStringLiteral("task-c"));
        QVERIFY(f.store.canMove());
        QCOMPARE(moved.count(), 0);
        QCOMPARE(refresh.count(), 1);

        // Working on the search again clears the old message.
        f.store.setQuery("gam");
        QVERIFY(f.store.error().isEmpty());
    }

    void aNewAttemptClearsTheOldError() {
        MoveFixture f;
        f.openWithResults();
        f.store.move();
        f.gateway.respond("moveWorklog", BridgeReply::failure("Conflict"));
        QVERIFY(!f.store.error().isEmpty());

        f.store.move();

        QVERIFY(f.store.error().isEmpty());
        QCOMPARE(f.gateway.count("moveWorklog"), 2);
    }

    void anUncertainMoveSaysItCouldNotBeConfirmed() {
        MoveFixture f;
        f.openWithResults();
        QSignalSpy refresh(&f.store, &WorklogMoveStore::refreshRequested);

        f.store.move();
        f.gateway.respond("moveWorklog", writeUncertain());

        QVERIFY(f.store.error().contains(QStringLiteral("could not be confirmed")));
        QVERIFY(f.store.error().contains(QStringLiteral("Check the reloaded history")));
        QVERIFY(!f.store.error().contains(QStringLiteral("timed out")));
        QCOMPARE(refresh.count(), 1);
    }

    void aFailedSearchKeepsTheOldResultsButBlocksTheMove() {
        MoveFixture f;
        f.openWithResults();
        f.store.setQuery("b");

        f.gateway.respond("moveCandidates", BridgeReply::failure("The tracker is unavailable"));

        QCOMPARE(f.store.error(), QStringLiteral("The tracker is unavailable"));
        QCOMPARE(f.store.candidates().size(), 2);
        QVERIFY(!f.store.searching());
        QVERIFY(!f.store.fresh());
        QVERIFY(!f.store.canMove());

        f.store.setQuery("be");
        QVERIFY(f.store.error().isEmpty());
        f.gateway.respond("moveCandidates", betaAndGamma());
        QVERIFY(f.store.canMove());
    }

    void anUnreadableSearchAnswerIsAnError() {
        MoveFixture f;
        QVERIFY(f.store.begin("w1"));

        f.gateway.respond("moveCandidates", BridgeReply::success(QJsonObject()));

        QVERIFY(f.store.error().contains(QStringLiteral("could not be read")));
        QVERIFY(!f.store.fresh());
        QVERIFY(!f.store.canMove());
    }

    void candidatesWithoutAnIdAreSkipped() {
        MoveFixture f;
        QVERIFY(f.store.begin("w1"));

        f.gateway.respond("moveCandidates", candidates({{"", "Nameless"}, {"task-b", "Beta"}}));

        QCOMPARE(f.store.candidates().size(), 1);
        QCOMPARE(idAt(f.store, 0), QStringLiteral("task-b"));
    }

    void cancellingDropsTheSearchAndSendsNothingMore() {
        MoveFixture f;
        QVERIFY(f.store.begin("w1"));

        f.store.cancel();
        f.gateway.respond("moveCandidates", betaAndGamma());
        f.store.setQuery("b");

        QVERIFY(f.store.candidates().isEmpty());
        QVERIFY(!f.store.searching());
        QVERIFY(!f.store.canMove());
        QCOMPARE(f.gateway.count("moveCandidates"), 1);
        f.store.move();
        QCOMPARE(f.gateway.count("moveWorklog"), 0);
    }

    void cancellingAnOpenMoveOnlyRefreshes() {
        MoveFixture f;
        f.openWithResults();
        QSignalSpy moved(&f.store, &WorklogMoveStore::moved);
        QSignalSpy refresh(&f.store, &WorklogMoveStore::refreshRequested);
        f.store.move();

        f.store.cancel();
        f.gateway.respond("moveWorklog", writeSucceeded());

        QCOMPARE(moved.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void aNewDialogIgnoresTheAnswerOfAnOlderMove() {
        MoveFixture f;
        f.openWithResults();
        f.store.move();
        QVERIFY(f.store.pending());

        QVERIFY(f.store.begin("w2"));

        QVERIFY(!f.store.pending());
        QSignalSpy moved(&f.store, &WorklogMoveStore::moved);
        QSignalSpy refresh(&f.store, &WorklogMoveStore::refreshRequested);
        f.gateway.respond("moveWorklog", BridgeReply::failure("Conflict"));
        QVERIFY(f.store.error().isEmpty());
        QCOMPARE(moved.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void aNewDialogStartsClean() {
        MoveFixture f;
        f.openWithResults();
        f.store.setQuery("g");
        f.gateway.respond("moveCandidates", betaAndGamma());
        f.store.move();
        f.gateway.respond("moveWorklog", BridgeReply::failure("Conflict"));
        QVERIFY(!f.store.error().isEmpty());

        QVERIFY(f.store.begin("w1"));

        QVERIFY(f.store.error().isEmpty());
        QVERIFY(f.store.query().isEmpty());
        QVERIFY(f.store.candidates().isEmpty());
        QVERIFY(f.store.selectedTaskId().isEmpty());
        QVERIFY(!f.store.canMove());
        QCOMPARE(f.gateway.lastArgs("moveCandidates"), (QStringList{"task-a", QString()}));
    }

    void answersAfterTheStoreIsGoneAreDropped() {
        MoveFixture f;
        auto *store = new WorklogMoveStore(&f.gateway, &f.history);
        QVERIFY(store->begin("w1"));
        f.gateway.respond("moveCandidates", betaAndGamma());
        store->move();
        store->setQuery("b");

        delete store;

        QVERIFY(f.gateway.respond("moveWorklog", writeSucceeded()));
        QVERIFY(f.gateway.respond("moveCandidates", betaAndGamma()));
    }
};

REGISTER_TEST(WorklogMoveTests)
#include "WorklogMoveTests.moc"
