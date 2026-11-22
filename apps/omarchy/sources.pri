# Shared by the app and the test binary. Every file under src/ belongs to the build, so
# features add files without editing a list.
QT += quick quickcontrols2 qml
CONFIG += c++20 warn_on

INCLUDEPATH += $$PWD/src $$PWD/../../crates/tracker-bridge/include

APP_SOURCES = $$files($$PWD/src/*.cpp, true)
APP_SOURCES -= $$PWD/src/main.cpp
SOURCES += $$APP_SOURCES
HEADERS += $$files($$PWD/src/*.h, true)

# The Rust bridge, built by bin/build with cargo before qmake runs.
CONFIG(release, debug|release): CARGO_PROFILE = release
else: CARGO_PROFILE = debug
isEmpty(CARGO_TARGET_DIR): CARGO_TARGET_DIR = $$PWD/../../target
LIBS += $$CARGO_TARGET_DIR/$$CARGO_PROFILE/libtracker_bridge.a -lpthread -ldl -lm
PRE_TARGETDEPS += $$CARGO_TARGET_DIR/$$CARGO_PROFILE/libtracker_bridge.a
