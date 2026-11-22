#pragma once

#include <QColor>
#include <QFileSystemWatcher>
#include <QHash>
#include <QObject>
#include <QTimer>

// Follows the current Omarchy theme live. Reads colors.toml from the theme directory and
// re-reads it when Omarchy swaps the directory or rewrites the file. Without Omarchy it
// falls back to a dark palette with Omarchy's default yellow accent.
class Theme : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool dark READ dark NOTIFY changed)
    Q_PROPERTY(QColor background READ background NOTIFY changed)
    Q_PROPERTY(QColor panel READ panel NOTIFY changed)
    Q_PROPERTY(QColor raised READ raised NOTIFY changed)
    Q_PROPERTY(QColor line READ line NOTIFY changed)
    Q_PROPERTY(QColor selection READ selection NOTIFY changed)
    Q_PROPERTY(QColor text READ text NOTIFY changed)
    Q_PROPERTY(QColor textDim READ textDim NOTIFY changed)
    Q_PROPERTY(QColor textFaint READ textFaint NOTIFY changed)
    Q_PROPERTY(QColor accent READ accent NOTIFY changed)
    Q_PROPERTY(QColor danger READ danger NOTIFY changed)
    Q_PROPERTY(int radius READ radius NOTIFY changed)
    Q_PROPERTY(QString fontFamily READ fontFamily CONSTANT)
public:
    // `stateDirectory` is Omarchy's ~/.local/state/omarchy/current. `radius` is Hyprland's
    // window rounding, which corners of panels and dialogs follow.
    explicit Theme(const QString &stateDirectory, int radius, QObject *parent = nullptr);

    static QString defaultStateDirectory();
    // Asks hyprctl for decoration:rounding. 0 without Hyprland, which is Omarchy's default too.
    static int hyprlandRounding();

    bool dark() const { return dark_; }
    QColor background() const { return color("background"); }
    QColor panel() const { return color("dark_background"); }
    QColor raised() const { return color("lighter_background"); }
    QColor line() const { return color("muted"); }
    QColor selection() const { return color("selection"); }
    QColor text() const { return color("foreground"); }
    QColor textDim() const { return color("light_foreground"); }
    QColor textFaint() const { return color("dark_foreground"); }
    QColor accent() const { return color("accent"); }
    QColor danger() const { return color("red"); }
    int radius() const { return radius_; }
    QString fontFamily() const;

    // The theme color for a task color name from TaskColor.
    Q_INVOKABLE QColor taskColor(const QString &name) const;

    void reload();

signals:
    void changed();

private:
    QColor color(const char *key) const;
    void watch();

    QString stateDirectory_;
    int radius_;
    bool dark_ = true;
    QHash<QString, QColor> colors_;
    QFileSystemWatcher watcher_;
    QTimer reloadTimer_;
};

namespace ThemeColors {
// Parses `key = "#rrggbb"` lines from colors.toml.
QHash<QString, QColor> parse(const QString &toml);
// Fills keys a theme leaves out from the keys it has, then from the fallback palette.
QHash<QString, QColor> complete(QHash<QString, QColor> colors, bool dark);
} // namespace ThemeColors
