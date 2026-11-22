#include <QSignalSpy>
#include <QTest>

#include "Shared/DisplayClock.h"
#include "TestRegistry.h"
#include "TestSupport.h"

using namespace TestSupport;

class DisplayClockTests : public QObject {
    Q_OBJECT
private slots:
    void usesTheSystemTimeWithoutASource() {
        DisplayClock clock({}, false);
        QVERIFY(qAbs(clock.now().secsTo(QDateTime::currentDateTime())) <= 1);
    }

    void usesTheGivenSource() {
        TestClock test("2026-11-10T12:00:00.000Z");
        QCOMPARE(test.clock()->now(), utc("2026-11-10T12:00:00.000Z"));
        test.set("2026-11-10T12:00:07.000Z");
        QCOMPARE(test.clock()->now(), utc("2026-11-10T12:00:07.000Z"));
    }

    void dayBoundsAreLocalMidnights() {
        // The test zone is UTC+1, so local midnight is 23:00 UTC the evening before.
        TestClock test("2026-11-10T12:00:00.000Z");
        QCOMPARE(test.clock()->dayStart().toUTC(), utc("2026-11-09T23:00:00.000Z"));
        QCOMPARE(test.clock()->dayEnd().toUTC(), utc("2026-11-10T23:00:00.000Z"));
    }

    void dayBoundsFollowTheLocalDateNotTheUtcDate() {
        // 23:30 UTC on the 10th is already the 11th for the user.
        TestClock test("2026-11-10T23:30:00.000Z");
        QCOMPARE(test.clock()->dayStart().toUTC(), utc("2026-11-10T23:00:00.000Z"));
        QCOMPARE(test.clock()->dayEnd().toUTC(), utc("2026-11-11T23:00:00.000Z"));
        // 22:30 UTC on the 10th is still the 10th.
        test.set("2026-11-10T22:30:00.000Z");
        QCOMPARE(test.clock()->dayStart().toUTC(), utc("2026-11-09T23:00:00.000Z"));
    }

    void everyTickEmitsTicked() {
        TestClock test;
        QSignalSpy ticked(test.clock(), &DisplayClock::ticked);
        test.clock()->tick();
        test.clock()->tick();
        QCOMPARE(ticked.count(), 2);
    }

    void dayChangedFiresOnceWhenTheDateCrossesMidnight() {
        TestClock test("2026-11-10T21:00:00.000Z");
        QSignalSpy ticked(test.clock(), &DisplayClock::ticked);
        QSignalSpy dayChanged(test.clock(), &DisplayClock::dayChanged);
        // 22:59 UTC is 23:59 local: still the same day.
        test.advanceTo("2026-11-10T22:59:00.000Z");
        QCOMPARE(dayChanged.count(), 0);
        // 23:01 UTC is 00:01 local on the next day.
        test.advanceTo("2026-11-10T23:01:00.000Z");
        QCOMPARE(dayChanged.count(), 1);
        test.advanceTo("2026-11-10T23:02:00.000Z");
        test.advanceTo("2026-11-11T10:00:00.000Z");
        QCOMPARE(dayChanged.count(), 1);
        QCOMPARE(ticked.count(), 4);
    }

    void dayChangedFiresAgainForTheFollowingMidnight() {
        TestClock test("2026-11-10T12:00:00.000Z");
        QSignalSpy dayChanged(test.clock(), &DisplayClock::dayChanged);
        test.advanceTo("2026-11-11T12:00:00.000Z");
        test.advanceTo("2026-11-12T12:00:00.000Z");
        QCOMPARE(dayChanged.count(), 2);
    }

    void tickedFiresBeforeDayChanged() {
        // Stores that update on ticked() see the new time before they handle the new day.
        TestClock test("2026-11-10T12:00:00.000Z");
        QStringList order;
        connect(test.clock(), &DisplayClock::ticked, this, [&] { order << "ticked"; });
        connect(test.clock(), &DisplayClock::dayChanged, this, [&] { order << "dayChanged"; });
        test.advanceTo("2026-11-11T12:00:00.000Z");
        QCOMPARE(order, QStringList({"ticked", "dayChanged"}));
    }

    void aTickingClockTicksOnItsOwn() {
        DisplayClock clock({}, true);
        QSignalSpy ticked(&clock, &DisplayClock::ticked);
        QTRY_VERIFY_WITH_TIMEOUT(ticked.count() >= 1, 3000);
    }

    void aClockWithoutTickingStaysQuiet() {
        DisplayClock clock({}, false);
        QSignalSpy ticked(&clock, &DisplayClock::ticked);
        QTest::qWait(1300);
        QCOMPARE(ticked.count(), 0);
    }
};

REGISTER_TEST(DisplayClockTests)
#include "DisplayClockTests.moc"
