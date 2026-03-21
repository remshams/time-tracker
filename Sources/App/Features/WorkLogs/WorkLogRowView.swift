import SwiftUI

struct WorkLogRowView: View {
    let entry: WorkLogEntry

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: AppSpacing.compact) {
            Text(entry.formattedTimeRange)
                .font(.body.monospacedDigit())
                .frame(width: WorkLogColumnWidths.time, alignment: .leading)

            Text(entry.formattedDuration)
                .font(.body.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: WorkLogColumnWidths.duration, alignment: .leading)

            if let description = entry.description {
                Text(description)
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            Spacer(minLength: 0)
        }
        .padding(.vertical, AppSpacing.tight)
    }
}
