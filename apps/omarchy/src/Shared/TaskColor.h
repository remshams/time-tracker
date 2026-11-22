#pragma once

#include <QString>

namespace TaskColor {
// The task's color name: blue, teal, green, orange, red, purple or pink. A 64-bit FNV-1a
// hash of the task ID picks it, the same as the macOS app and the Omarchy bar widget, so
// a task has one color on every client.
QString nameFor(const QString &taskId);
} // namespace TaskColor
