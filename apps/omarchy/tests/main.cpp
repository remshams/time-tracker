#include <QGuiApplication>
#include <QTest>

#include "TestRegistry.h"

namespace TestRegistry {
std::vector<Factory> &factories() {
    static std::vector<Factory> list;
    return list;
}
} // namespace TestRegistry

int main(int argc, char *argv[]) {
    qputenv("QT_QPA_PLATFORM", "offscreen");
    // A zone with a fixed offset of one hour, so local-time tests give the same answer on
    // every machine and still tell local time from UTC.
    qputenv("TZ", "CET-1");
    QGuiApplication app(argc, argv);
    int failures = 0;
    for (const TestRegistry::Factory &factory : TestRegistry::factories()) {
        QObject *test = factory();
        failures += QTest::qExec(test, argc, argv);
        delete test;
    }
    return failures == 0 ? 0 : 1;
}
