import SwiftUI

@main
struct RuSToatApp: App {
    @State private var model: AppModel

    init() {
        initLogging()
        _model = State(initialValue: AppModel.live())
    }

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .task { await model.restore() }
        }
    }
}
