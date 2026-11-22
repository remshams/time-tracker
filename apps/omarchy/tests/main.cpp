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
    QGuiApplication app(argc, argv);
    int failures = 0;
    for (const TestRegistry::Factory &factory : TestRegistry::factories()) {
        QObject *test = factory();
        failures += QTest::qExec(test, argc, argv);
        delete test;
    }
    return failures == 0 ? 0 : 1;
}
