#include <QDir>
#include <QFile>
#include <QSaveFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

#include "Shared/Theme.h"
#include "TestRegistry.h"

namespace {

void writeFile(const QString &path, const QString &text) {
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    QVERIFY2(file.open(QIODevice::WriteOnly | QIODevice::Truncate), qPrintable(path));
    file.write(text.toUtf8());
}

QString colorsPath(const QString &stateDirectory) {
    return stateDirectory + QStringLiteral("/theme/colors.toml");
}

// The same mix the theme derives its surfaces with.
QColor mix(const QColor &background, const QColor &foreground, qreal amount) {
    return QColor::fromRgbF(background.redF() + (foreground.redF() - background.redF()) * amount,
                            background.greenF() + (foreground.greenF() - background.greenF()) * amount,
                            background.blueF() + (foreground.blueF() - background.blueF()) * amount);
}

// A fake hyprctl on PATH for the duration of a test.
class FakeHyprctl {
public:
    FakeHyprctl(const QString &script) {
        const QString path = dir_.filePath(QStringLiteral("hyprctl"));
        QFile file(path);
        if (file.open(QIODevice::WriteOnly)) {
            file.write(("#!/bin/sh\n" + script + "\n").toUtf8());
            file.setPermissions(QFile::permissions(path) | QFile::ExeOwner);
        }
        previousPath_ = qgetenv("PATH");
        qputenv("PATH", (dir_.path() + QStringLiteral(":/usr/bin:/bin")).toUtf8());
    }
    ~FakeHyprctl() { qputenv("PATH", previousPath_); }

private:
    QTemporaryDir dir_;
    QByteArray previousPath_;
};

} // namespace

class ThemeTests : public QObject {
    Q_OBJECT
private slots:
    // ThemeColors::parse

    void parseReadsQuotedHexColors() {
        const QHash<QString, QColor> colors = ThemeColors::parse(
            QStringLiteral("accent = \"#112233\"\nbackground='#445566'\n  foreground   =   \"#aabbcc\"\n"));
        QCOMPARE(colors.size(), 3);
        QCOMPARE(colors.value("accent"), QColor("#112233"));
        QCOMPARE(colors.value("background"), QColor("#445566"));
        QCOMPARE(colors.value("foreground"), QColor("#aabbcc"));
    }

    void parseReadsColorsWithAnAlphaChannel() {
        const QHash<QString, QColor> colors = ThemeColors::parse(QStringLiteral("selection = \"#80112233\"\n"));
        QCOMPARE(colors.value("selection"), QColor("#80112233"));
    }

    void parseSkipsLinesItCannotUse() {
        const QHash<QString, QColor> colors = ThemeColors::parse(QStringLiteral("# accent = \"#111111\"\n"
                                                                                "name = \"Tokyo Night\"\n"
                                                                                "short = \"#12345\"\n"
                                                                                "odd = \"#1234567\"\n"
                                                                                "color0 = \"#222222\"\n"
                                                                                "unquoted = #333333\n"
                                                                                "[section]\n"
                                                                                "red = \"#f7768e\"\n"));
        QCOMPARE(colors.size(), 1);
        QVERIFY(colors.contains("red"));
    }

    void parseKeepsTheLastValueOfARepeatedKey() {
        const QHash<QString, QColor> colors = ThemeColors::parse(QStringLiteral("a = \"#111111\"\na = \"#222222\"\n"));
        QCOMPARE(colors.value("a"), QColor("#222222"));
    }

    void parseOfNothingIsEmpty() { QVERIFY(ThemeColors::parse(QString()).isEmpty()); }

    // ThemeColors::complete

    void completeFillsAnEmptyThemeFromTheDarkFallback() {
        const QHash<QString, QColor> colors = ThemeColors::complete({}, true);
        QCOMPARE(colors.value("background"), QColor("#1a1b26"));
        QCOMPARE(colors.value("foreground"), QColor("#c0caf5"));
        QCOMPARE(colors.value("accent"), QColor("#FFD60A"));
        QCOMPARE(colors.value("red"), QColor("#f7768e"));
        QCOMPARE(colors.value("yellow"), QColor("#e0af68"));
        QCOMPARE(colors.value("orange"), QColor("#ff9e64"));
        QCOMPARE(colors.value("green"), QColor("#9ece6a"));
        QCOMPARE(colors.value("cyan"), QColor("#7dcfff"));
        QCOMPARE(colors.value("blue"), QColor("#7aa2f7"));
        QCOMPARE(colors.value("magenta"), QColor("#bb9af7"));
    }

    void completeFillsAnEmptyThemeFromTheLightFallback() {
        const QHash<QString, QColor> colors = ThemeColors::complete({}, false);
        QCOMPARE(colors.value("background"), QColor("#faf4ed"));
        QCOMPARE(colors.value("foreground"), QColor("#575279"));
        QCOMPARE(colors.value("accent"), QColor("#56949f"));
        QCOMPARE(colors.value("red"), QColor("#b4637a"));
        QCOMPARE(colors.value("yellow"), QColor("#ea9d34"));
        QCOMPARE(colors.value("orange"), QColor("#cf8057"));
        QCOMPARE(colors.value("green"), QColor("#286983"));
        QCOMPARE(colors.value("cyan"), QColor("#d7827e"));
        QCOMPARE(colors.value("blue"), QColor("#56949f"));
        QCOMPARE(colors.value("magenta"), QColor("#907aa9"));
    }

    void completeKeepsWhatTheThemeDefines() {
        const QHash<QString, QColor> colors = ThemeColors::complete(
            {{"accent", QColor("#010203")}, {"background", QColor("#101010")}, {"foreground", QColor("#f0f0f0")}},
            true);
        QCOMPARE(colors.value("accent"), QColor("#010203"));
        QCOMPARE(colors.value("background"), QColor("#101010"));
        QCOMPARE(colors.value("foreground"), QColor("#f0f0f0"));
        // Keys the theme leaves out still come from the fallback.
        QCOMPARE(colors.value("red"), QColor("#f7768e"));
    }

    void completeDerivesSurfacesForADarkTheme() {
        const QColor background("#101010");
        const QColor foreground("#f0f0f0");
        const QHash<QString, QColor> colors =
            ThemeColors::complete({{"background", background}, {"foreground", foreground}}, true);
        QCOMPARE(colors.value("dark_background"), background.darker(115));
        QCOMPARE(colors.value("lighter_background"), mix(background, foreground, 0.06));
        QCOMPARE(colors.value("muted"), mix(background, foreground, 0.22));
        QCOMPARE(colors.value("selection"), mix(background, foreground, 0.14));
        QCOMPARE(colors.value("light_foreground"), mix(background, foreground, 0.72));
        QCOMPARE(colors.value("dark_foreground"), mix(background, foreground, 0.5));
    }

    void completeDerivesTheDarkBackgroundOfALightThemeByMixing() {
        const QColor background("#fafafa");
        const QColor foreground("#202020");
        const QHash<QString, QColor> colors =
            ThemeColors::complete({{"background", background}, {"foreground", foreground}}, false);
        QCOMPARE(colors.value("dark_background"), mix(background, foreground, 0.05));
    }

    void completeKeepsSurfacesTheThemeDefines() {
        const QHash<QString, QColor> colors = ThemeColors::complete({{"dark_background", QColor("#000001")},
                                                                     {"lighter_background", QColor("#000002")},
                                                                     {"muted", QColor("#000003")},
                                                                     {"selection", QColor("#000004")},
                                                                     {"light_foreground", QColor("#000005")},
                                                                     {"dark_foreground", QColor("#000006")}},
                                                                    true);
        QCOMPARE(colors.value("dark_background"), QColor("#000001"));
        QCOMPARE(colors.value("lighter_background"), QColor("#000002"));
        QCOMPARE(colors.value("muted"), QColor("#000003"));
        QCOMPARE(colors.value("selection"), QColor("#000004"));
        QCOMPARE(colors.value("light_foreground"), QColor("#000005"));
        QCOMPARE(colors.value("dark_foreground"), QColor("#000006"));
    }

    // Theme

    void readsTheThemeFile() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()),
                  QStringLiteral(
                      "background = \"#101010\"\nforeground = \"#f0f0f0\"\naccent = \"#ff0000\"\nred = \"#aa0000\"\n"
                      "dark_background = \"#000000\"\nlighter_background = \"#202020\"\nmuted = \"#303030\"\n"
                      "selection = \"#404040\"\nlight_foreground = \"#e0e0e0\"\ndark_foreground = \"#909090\"\n"));
        Theme theme(dir.path(), 8);
        QVERIFY(theme.dark());
        QCOMPARE(theme.radius(), 8);
        QCOMPARE(theme.background(), QColor("#101010"));
        QCOMPARE(theme.text(), QColor("#f0f0f0"));
        QCOMPARE(theme.accent(), QColor("#ff0000"));
        QCOMPARE(theme.danger(), QColor("#aa0000"));
        QCOMPARE(theme.panel(), QColor("#000000"));
        QCOMPARE(theme.raised(), QColor("#202020"));
        QCOMPARE(theme.line(), QColor("#303030"));
        QCOMPARE(theme.selection(), QColor("#404040"));
        QCOMPARE(theme.textDim(), QColor("#e0e0e0"));
        QCOMPARE(theme.textFaint(), QColor("#909090"));
    }

    void detectsALightTheme() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("mode = \"light\"\naccent = \"#112233\"\n"));
        Theme theme(dir.path(), 0);
        QVERIFY(!theme.dark());
        // The missing keys come from the light fallback.
        QCOMPARE(theme.background(), QColor("#faf4ed"));
        QCOMPARE(theme.accent(), QColor("#112233"));
    }

    void detectsALightThemeWithSingleQuotes() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("  mode = 'light'\n"));
        Theme theme(dir.path(), 0);
        QVERIFY(!theme.dark());
    }

    void aDarkOrCommentedModeStaysDark() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("mode = \"dark\"\n# mode = \"light\"\n"));
        Theme theme(dir.path(), 0);
        QVERIFY(theme.dark());
    }

    void aMissingFileFallsBackToOmarchysDefaults() {
        QTemporaryDir dir;
        Theme theme(dir.path() + QStringLiteral("/missing"), 4);
        QVERIFY(theme.dark());
        QCOMPARE(theme.background(), QColor("#1a1b26"));
        QCOMPARE(theme.accent(), QColor("#FFD60A"));
        QVERIFY(theme.panel().isValid());
        QVERIFY(theme.line().isValid());
    }

    void exposesAMonospaceFontFamily() {
        QTemporaryDir dir;
        Theme theme(dir.path(), 0);
        QVERIFY(!theme.fontFamily().isEmpty());
    }

    void mapsTaskColorNamesToThemeColors() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()),
                  QStringLiteral("blue = \"#000001\"\ncyan = \"#000002\"\ngreen = \"#000003\"\norange = \"#000004\"\n"
                                 "red = \"#000005\"\nmagenta = \"#000006\"\nyellow = \"#000007\"\n"));
        Theme theme(dir.path(), 0);
        QCOMPARE(theme.taskColor("blue"), QColor("#000001"));
        QCOMPARE(theme.taskColor("teal"), QColor("#000002"));
        QCOMPARE(theme.taskColor("green"), QColor("#000003"));
        QCOMPARE(theme.taskColor("orange"), QColor("#000004"));
        QCOMPARE(theme.taskColor("red"), QColor("#000005"));
        QCOMPARE(theme.taskColor("purple"), QColor("#000006"));
        QCOMPARE(theme.taskColor("pink"), QColor("#000007"));
    }

    void anUnknownTaskColorNameUsesBlue() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("blue = \"#000001\"\n"));
        Theme theme(dir.path(), 0);
        QCOMPARE(theme.taskColor("chartreuse"), QColor("#000001"));
        QCOMPARE(theme.taskColor(QString()), QColor("#000001"));
    }

    void reloadEmitsChanged() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#111111\"\n"));
        Theme theme(dir.path(), 0);
        QSignalSpy changed(&theme, &Theme::changed);
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#222222\"\n"));
        theme.reload();
        QCOMPARE(changed.count(), 1);
        QCOMPARE(theme.accent(), QColor("#222222"));
    }

    void followsAnEditOfColorsToml() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#111111\"\n"));
        Theme theme(dir.path(), 0);
        QSignalSpy changed(&theme, &Theme::changed);
        QCOMPARE(theme.accent(), QColor("#111111"));
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#222222\"\n"));
        QTRY_COMPARE(theme.accent(), QColor("#222222"));
        QVERIFY(changed.count() >= 1);
        // The watch survives the first change.
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#333333\"\n"));
        QTRY_COMPARE(theme.accent(), QColor("#333333"));
    }

    void followsAFileThatIsReplaced() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#111111\"\n"));
        Theme theme(dir.path(), 0);
        // Omarchy swaps the file; the old inode and its watch disappear.
        for (const char *accent : {"#222222", "#333333"}) {
            QSaveFile file(colorsPath(dir.path()));
            QVERIFY(file.open(QIODevice::WriteOnly));
            file.write(QByteArray("accent = \"") + accent + "\"\n");
            QVERIFY(file.commit());
            QTRY_COMPARE(theme.accent(), QColor(accent));
        }
    }

    void followsAThemeThatAppearsLater() {
        QTemporaryDir dir;
        Theme theme(dir.path(), 0);
        QCOMPARE(theme.accent(), QColor("#FFD60A"));
        writeFile(colorsPath(dir.path()), QStringLiteral("mode = \"light\"\naccent = \"#123456\"\n"));
        QTRY_COMPARE(theme.accent(), QColor("#123456"));
        QVERIFY(!theme.dark());
    }

    void followsAThemeDirectorySwap() {
        QTemporaryDir dir;
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#111111\"\n"));
        Theme theme(dir.path(), 0);
        QVERIFY(QDir(dir.path() + QStringLiteral("/theme")).removeRecursively());
        writeFile(colorsPath(dir.path()), QStringLiteral("accent = \"#444444\"\n"));
        QTRY_COMPARE(theme.accent(), QColor("#444444"));
    }

    // Static helpers

    void theDefaultStateDirectoryIsOmarchysCurrentTheme() {
        QVERIFY(Theme::defaultStateDirectory().endsWith(QStringLiteral("/.local/state/omarchy/current")));
    }

    void roundingComesFromHyprctl() {
        FakeHyprctl hyprctl(R"(echo '{"option":"decoration:rounding","int":12,"set":true}')");
        QCOMPARE(Theme::hyprlandRounding(), 12);
    }

    void roundingIsZeroWhenHyprctlFails() {
        FakeHyprctl hyprctl("echo '{\"int\":12}'\nexit 1");
        QCOMPARE(Theme::hyprlandRounding(), 0);
    }

    void roundingIsZeroWhenHyprctlAnswersNonsense() {
        FakeHyprctl hyprctl("echo 'nothing'");
        QCOMPARE(Theme::hyprlandRounding(), 0);
    }

    void roundingNeverGoesNegative() {
        FakeHyprctl hyprctl("echo '{\"int\":-3}'");
        QCOMPARE(Theme::hyprlandRounding(), 0);
    }

    void roundingIsZeroWithoutHyprctl() {
        const QByteArray previous = qgetenv("PATH");
        QTemporaryDir empty;
        qputenv("PATH", empty.path().toUtf8());
        const int rounding = Theme::hyprlandRounding();
        qputenv("PATH", previous);
        QCOMPARE(rounding, 0);
    }
};

REGISTER_TEST(ThemeTests)
#include "ThemeTests.moc"
