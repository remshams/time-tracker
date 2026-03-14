import Testing
@testable import App

@Test func contentViewModuleLoads() {
    let view = ContentView()
    #expect(String(describing: type(of: view)) == "ContentView")
}