#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>

#include "App/AppController.h"
#include "Bridge/RustTrackerGateway.h"
#include "Shared/Theme.h"

#ifndef TT_GIT_REVISION
#define TT_GIT_REVISION "unknown"
#endif

int main(int argc, char *argv[]) {
    QGuiApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("tt-desktop"));
    QGuiApplication::setApplicationDisplayName(QStringLiteral("Time Tracker"));
    QGuiApplication::setOrganizationName(QStringLiteral("tt-desktop"));
    // Qt.application.version in QML. The install script passes the git revision.
    QGuiApplication::setApplicationVersion(QStringLiteral(TT_GIT_REVISION));
    // Lets Hyprland and the launcher match the window to tt-desktop.desktop and its icon.
    QGuiApplication::setDesktopFileName(QStringLiteral("tt-desktop"));
    QQuickStyle::setStyle(QStringLiteral("Material"));

    RustTrackerGateway gateway;
    Theme theme(Theme::defaultStateDirectory(), Theme::hyprlandRounding());
    AppController controller(&gateway, &theme);

    QQmlApplicationEngine engine;
    controller.expose(engine.rootContext());
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(1); },
                     Qt::QueuedConnection);
    engine.load(QUrl(QStringLiteral("qrc:/App/Main.qml")));
    controller.start();
    return QGuiApplication::exec();
}
