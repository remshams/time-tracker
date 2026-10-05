import SwiftUI

struct TrackerElapsedText: View {
    @ObservedObject var timer: TrackerTimerStore

    var body: some View { Text(timer.text) }
}
