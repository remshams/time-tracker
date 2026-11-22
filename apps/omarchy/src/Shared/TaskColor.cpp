#include "Shared/TaskColor.h"

#include <array>
#include <cstdint>

namespace TaskColor {

QString nameFor(const QString &taskId) {
    static const std::array<const char *, 7> names{"blue", "teal", "green", "orange", "red", "purple", "pink"};
    std::uint64_t hash = 0xcbf29ce484222325ULL;
    for (const char byte : taskId.toUtf8()) {
        hash ^= static_cast<std::uint8_t>(byte);
        hash *= 0x100000001b3ULL;
    }
    return QString::fromLatin1(names[hash % names.size()]);
}

} // namespace TaskColor
