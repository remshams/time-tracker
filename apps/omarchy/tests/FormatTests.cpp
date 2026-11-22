#include <QLocale>
#include <QTest>

#include "Shared/Format.h"
#include "TestRegistry.h"
#include "TestSupport.h"

using namespace TestSupport;

class FormatTests : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        // Day names come from the locale; pin it so the expected text is the same everywhere.
        QLocale::setDefault(QLocale(QLocale::English, QLocale::UnitedStates));
    }

    void durationShowsHoursMinutesAndSeconds() {
        QCOMPARE(Format::duration(4320), QStringLiteral("1:12:00"));
        QCOMPARE(Format::duration(0), QStringLiteral("0:00:00"));
        QCOMPARE(Format::duration(59), QStringLiteral("0:00:59"));
        QCOMPARE(Format::duration(61), QStringLiteral("0:01:01"));
        QCOMPARE(Format::duration(3599), QStringLiteral("0:59:59"));
        QCOMPARE(Format::duration(3600), QStringLiteral("1:00:00"));
        QCOMPARE(Format::duration(360000 + 5), QStringLiteral("100:00:05"));
    }

    void negativeDurationsCountAsZero() {
        QCOMPARE(Format::duration(-1), QStringLiteral("0:00:00"));
        QCOMPARE(Format::duration(-4320), QStringLiteral("0:00:00"));
        QCOMPARE(Format::hoursMinutes(-90), QStringLiteral("0:00"));
        QCOMPARE(Format::secondsSuffix(-75), QStringLiteral(":00"));
    }

    void hoursMinutesLeavesOutTheSeconds() {
        QCOMPARE(Format::hoursMinutes(4320), QStringLiteral("1:12"));
        QCOMPARE(Format::hoursMinutes(59), QStringLiteral("0:00"));
        QCOMPARE(Format::hoursMinutes(7 * 60 + 7), QStringLiteral("0:07"));
        QCOMPARE(Format::hoursMinutes(36000), QStringLiteral("10:00"));
    }

    void secondsSuffixWrapsEveryMinute() {
        QCOMPARE(Format::secondsSuffix(0), QStringLiteral(":00"));
        QCOMPARE(Format::secondsSuffix(5), QStringLiteral(":05"));
        QCOMPARE(Format::secondsSuffix(60), QStringLiteral(":00"));
        QCOMPARE(Format::secondsSuffix(119), QStringLiteral(":59"));
    }

    void clockTimeUsesLocalTime() {
        // The test zone is UTC+1.
        QCOMPARE(Format::clockTime(utc("2026-11-10T14:30:00.000Z")), QStringLiteral("15:30"));
        QCOMPARE(Format::clockTime(utc("2026-11-10T23:05:00.000Z")), QStringLiteral("00:05"));
    }

    void dayLabelNamesTodayAndYesterday() {
        const QDate today(2026, 11, 10);
        QCOMPARE(Format::dayLabel(today, today), QStringLiteral("Today"));
        QCOMPARE(Format::dayLabel(QDate(2026, 11, 9), today), QStringLiteral("Yesterday"));
        QCOMPARE(Format::dayLabel(QDate(2025, 12, 31), QDate(2026, 1, 1)), QStringLiteral("Yesterday"));
    }

    void dayLabelOmitsTheYearWithinTheCurrentYear() {
        QCOMPARE(Format::dayLabel(QDate(2026, 11, 3), QDate(2026, 11, 10)), QStringLiteral("Tue 3 Nov"));
        // A day after today is not special either.
        QCOMPARE(Format::dayLabel(QDate(2026, 11, 11), QDate(2026, 11, 10)), QStringLiteral("Wed 11 Nov"));
    }

    void dayLabelAddsTheYearForOtherYears() {
        QCOMPARE(Format::dayLabel(QDate(2025, 12, 30), QDate(2026, 11, 10)), QStringLiteral("Tue 30 Dec 2025"));
    }

    void lastWorkedSaysNeverWithoutAStart() {
        QCOMPARE(Format::lastWorked(std::nullopt, QDate(2026, 11, 10)), QStringLiteral("Never"));
    }

    void lastWorkedShowsTheTimeForToday() {
        QCOMPARE(Format::lastWorked(utc("2026-11-10T12:12:00.000Z"), QDate(2026, 11, 10)),
                 QStringLiteral("Today 13:12"));
    }

    void lastWorkedTreatsFutureStartsAsToday() {
        QCOMPARE(Format::lastWorked(utc("2026-11-11T09:00:00.000Z"), QDate(2026, 11, 10)),
                 QStringLiteral("Today 10:00"));
    }

    void lastWorkedUsesTheLocalDayNotTheUtcDay() {
        // 23:30 UTC on the 9th is 00:30 local on the 10th.
        QCOMPARE(Format::lastWorked(utc("2026-11-09T23:30:00.000Z"), QDate(2026, 11, 10)),
                 QStringLiteral("Today 00:30"));
        // 22:30 UTC on the 9th is 23:30 local on the 9th.
        QCOMPARE(Format::lastWorked(utc("2026-11-09T22:30:00.000Z"), QDate(2026, 11, 10)), QStringLiteral("Yesterday"));
    }

    void lastWorkedCountsDaysAfterYesterday() {
        QCOMPARE(Format::lastWorked(utc("2026-11-08T12:00:00.000Z"), QDate(2026, 11, 10)),
                 QStringLiteral("2 days ago"));
        QCOMPARE(Format::lastWorked(utc("2026-11-01T12:00:00.000Z"), QDate(2026, 11, 10)),
                 QStringLiteral("9 days ago"));
    }
};

REGISTER_TEST(FormatTests)
#include "FormatTests.moc"
