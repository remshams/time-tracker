#include "Shared/Theme.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QFontDatabase>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QRegularExpression>
#include <QStandardPaths>

namespace ThemeColors {

QHash<QString, QColor> parse(const QString &toml) {
    static const QRegularExpression line(
        QStringLiteral(R"(^\s*([a-z_]+)\s*=\s*["'](#[0-9A-Fa-f]{6}|#[0-9A-Fa-f]{8})["'])"),
        QRegularExpression::MultilineOption);
    QHash<QString, QColor> colors;
    for (const QRegularExpressionMatch &match : line.globalMatch(toml)) {
        colors.insert(match.captured(1), QColor(match.captured(2)));
    }
    return colors;
}

QHash<QString, QColor> complete(QHash<QString, QColor> colors, bool dark) {
    // Omarchy's own defaults when no theme is there: a dark surface and the yellow accent.
    static const QHash<QString, QColor> darkFallback{
        {QStringLiteral("background"), QColor("#1a1b26")}, {QStringLiteral("foreground"), QColor("#c0caf5")},
        {QStringLiteral("accent"), QColor("#FFD60A")},     {QStringLiteral("red"), QColor("#f7768e")},
        {QStringLiteral("yellow"), QColor("#e0af68")},     {QStringLiteral("orange"), QColor("#ff9e64")},
        {QStringLiteral("green"), QColor("#9ece6a")},      {QStringLiteral("cyan"), QColor("#7dcfff")},
        {QStringLiteral("blue"), QColor("#7aa2f7")},       {QStringLiteral("magenta"), QColor("#bb9af7")},
    };
    static const QHash<QString, QColor> lightFallback{
        {QStringLiteral("background"), QColor("#faf4ed")}, {QStringLiteral("foreground"), QColor("#575279")},
        {QStringLiteral("accent"), QColor("#56949f")},     {QStringLiteral("red"), QColor("#b4637a")},
        {QStringLiteral("yellow"), QColor("#ea9d34")},     {QStringLiteral("orange"), QColor("#cf8057")},
        {QStringLiteral("green"), QColor("#286983")},      {QStringLiteral("cyan"), QColor("#d7827e")},
        {QStringLiteral("blue"), QColor("#56949f")},       {QStringLiteral("magenta"), QColor("#907aa9")},
    };
    const QHash<QString, QColor> &fallback = dark ? darkFallback : lightFallback;
    for (auto it = fallback.cbegin(); it != fallback.cend(); ++it) {
        if (!colors.contains(it.key())) {
            colors.insert(it.key(), it.value());
        }
    }
    const QColor background = colors.value(QStringLiteral("background"));
    const QColor foreground = colors.value(QStringLiteral("foreground"));
    // Older themes only define the base pair; derive the surfaces from it.
    auto mix = [&](qreal amount) {
        return QColor::fromRgbF(background.redF() + (foreground.redF() - background.redF()) * amount,
                                background.greenF() + (foreground.greenF() - background.greenF()) * amount,
                                background.blueF() + (foreground.blueF() - background.blueF()) * amount);
    };
    colors.insert(QStringLiteral("dark_background"),
                  colors.value(QStringLiteral("dark_background"), dark ? background.darker(115) : mix(0.05)));
    colors.insert(QStringLiteral("lighter_background"), colors.value(QStringLiteral("lighter_background"), mix(0.06)));
    colors.insert(QStringLiteral("muted"), colors.value(QStringLiteral("muted"), mix(0.22)));
    colors.insert(QStringLiteral("selection"), colors.value(QStringLiteral("selection"), mix(0.14)));
    colors.insert(QStringLiteral("light_foreground"), colors.value(QStringLiteral("light_foreground"), mix(0.72)));
    colors.insert(QStringLiteral("dark_foreground"), colors.value(QStringLiteral("dark_foreground"), mix(0.5)));
    return colors;
}

} // namespace ThemeColors

Theme::Theme(const QString &stateDirectory, int radius, QObject *parent)
    : QObject(parent), stateDirectory_(stateDirectory), radius_(radius) {
    // A theme switch replaces files and symlinks in several steps; wait for it to settle.
    reloadTimer_.setSingleShot(true);
    reloadTimer_.setInterval(150);
    connect(&reloadTimer_, &QTimer::timeout, this, &Theme::reload);
    connect(&watcher_, &QFileSystemWatcher::fileChanged, &reloadTimer_, qOverload<>(&QTimer::start));
    connect(&watcher_, &QFileSystemWatcher::directoryChanged, &reloadTimer_, qOverload<>(&QTimer::start));
    reload();
}

QString Theme::defaultStateDirectory() {
    return QDir::homePath() + QStringLiteral("/.local/state/omarchy/current");
}

int Theme::hyprlandRounding() {
    QProcess hyprctl;
    hyprctl.start(QStringLiteral("hyprctl"),
                  {QStringLiteral("getoption"), QStringLiteral("decoration:rounding"), QStringLiteral("-j")});
    if (!hyprctl.waitForFinished(1000) || hyprctl.exitCode() != 0) {
        return 0;
    }
    return qMax(0, QJsonDocument::fromJson(hyprctl.readAllStandardOutput()).object().value("int").toInt());
}

QString Theme::fontFamily() const {
    // Omarchy sets one system font, monospace by default, and uses it everywhere.
    return QFontDatabase::systemFont(QFontDatabase::FixedFont).family();
}

QColor Theme::taskColor(const QString &name) const {
    // The same mapping as the Omarchy bar widget. A few stock themes have no orange.
    static const QHash<QString, QString> keys{
        {QStringLiteral("blue"), QStringLiteral("blue")},   {QStringLiteral("teal"), QStringLiteral("cyan")},
        {QStringLiteral("green"), QStringLiteral("green")}, {QStringLiteral("orange"), QStringLiteral("orange")},
        {QStringLiteral("red"), QStringLiteral("red")},     {QStringLiteral("purple"), QStringLiteral("magenta")},
        {QStringLiteral("pink"), QStringLiteral("yellow")},
    };
    const QString key = keys.value(name, QStringLiteral("blue"));
    return colors_.value(key);
}

void Theme::reload() {
    QFile file(stateDirectory_ + QStringLiteral("/theme/colors.toml"));
    QString toml;
    if (file.open(QIODevice::ReadOnly)) {
        toml = QString::fromUtf8(file.readAll());
    }
    static const QRegularExpression modeLine(QStringLiteral(R"(^\s*mode\s*=\s*["']light["'])"),
                                             QRegularExpression::MultilineOption);
    dark_ = !modeLine.match(toml).hasMatch();
    colors_ = ThemeColors::complete(ThemeColors::parse(toml), dark_);
    watch();
    emit changed();
}

QColor Theme::color(const char *key) const {
    return colors_.value(QString::fromLatin1(key));
}

void Theme::watch() {
    // A swap replaces the watched paths, so the watcher drops them; add them again each time.
    const QStringList paths{stateDirectory_, stateDirectory_ + QStringLiteral("/theme"),
                            stateDirectory_ + QStringLiteral("/theme/colors.toml")};
    for (const QString &path : paths) {
        if (QFileInfo::exists(path) && !watcher_.files().contains(path) && !watcher_.directories().contains(path)) {
            watcher_.addPath(path);
        }
    }
}
