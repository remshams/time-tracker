TEMPLATE = app
TARGET = tt-desktop

include(sources.pri)

SOURCES += $$PWD/src/main.cpp

# The install script sets TT_GIT_REVISION. main.cpp shows it as Qt.application.version.
TT_GIT_REVISION = $$(TT_GIT_REVISION)
isEmpty(TT_GIT_REVISION): TT_GIT_REVISION = unknown
DEFINES += TT_GIT_REVISION=\\\"$$TT_GIT_REVISION\\\"

# Every QML file under src/ is compiled into the binary at the same relative path.
qml.files = $$files($$PWD/src/*.qml, true) $$files($$PWD/src/qmldir, true)
qml.base = $$PWD/src
qml.prefix = /
RESOURCES += qml

target.path = /usr/bin
INSTALLS += target
