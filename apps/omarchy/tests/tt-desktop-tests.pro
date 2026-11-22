TEMPLATE = app
TARGET = tt-desktop-tests
QT += testlib
CONFIG += testcase

include(../sources.pri)

# Every test file registers its test class with TestRegistry.
SOURCES += $$files($$PWD/*.cpp)
HEADERS += $$files($$PWD/*.h)
INCLUDEPATH += $$PWD
