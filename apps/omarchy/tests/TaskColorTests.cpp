#include <QTest>

#include "Shared/TaskColor.h"
#include "TestRegistry.h"

class TaskColorTests : public QObject {
    Q_OBJECT
private slots:
    // The same vectors as the macOS app and the bar widget, so a task keeps its color everywhere.
    void matchesTheOtherClients() {
        QCOMPARE(TaskColor::nameFor(QStringLiteral("task-one")), QStringLiteral("green"));
        QCOMPARE(TaskColor::nameFor(QStringLiteral("task-two")), QStringLiteral("blue"));
        QCOMPARE(TaskColor::nameFor(QString()), QStringLiteral("green"));
    }
};

REGISTER_TEST(TaskColorTests)
#include "TaskColorTests.moc"
