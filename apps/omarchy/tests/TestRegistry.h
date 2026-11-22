#pragma once

#include <QObject>
#include <functional>
#include <vector>

// Collects the test classes so one binary runs them all.
namespace TestRegistry {
using Factory = std::function<QObject *()>;
std::vector<Factory> &factories();

template <typename T> struct Register {
    Register() {
        factories().push_back([] { return new T; });
    }
};
} // namespace TestRegistry

#define REGISTER_TEST(Class) static TestRegistry::Register<Class> registered##Class;
