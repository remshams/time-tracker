import SwiftUI

struct ContentView: View {
    @StateObject private var viewModel: TaskListViewModel

    init(viewModel: TaskListViewModel) {
        _viewModel = StateObject(wrappedValue: viewModel)
    }

    var body: some View {
        TaskListView(viewModel: viewModel)
    }
}
