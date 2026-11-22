TEMPLATE = app
TARGET = tt-desktop

include(sources.pri)

SOURCES += $$PWD/src/main.cpp

# Every QML file under src/ is compiled into the binary at the same relative path.
qml.files = $$files($$PWD/src/*.qml, true)
qml.base = $$PWD/src
qml.prefix = /
RESOURCES += qml

target.path = /usr/bin
INSTALLS += target
