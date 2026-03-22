import SwiftUI

struct PlaceholderView: View {
  let systemImage: String
  let title: String
  let description: String

  var body: some View {
    VStack(spacing: AppSpacing.compact) {
      Image(systemName: systemImage)
        .font(.title2)
        .accessibilityHidden(true)
      Text(title)
        .font(.headline)
      Text(description)
        .font(.subheadline)
        .foregroundStyle(.secondary)
    }
  }
}
