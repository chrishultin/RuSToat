import SwiftUI

@main
struct RuSToatApp: App {
    @State private var model: AppModel
    @Environment(\.scenePhase) private var scenePhase

    init() {
        initLogging()
        _model = State(initialValue: AppModel.live())
    }

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .task { await model.restore() }
        }
        .onChange(of: scenePhase) { _, phase in
            // Inactive (the app switcher, a notification pulled down) keeps
            // the connection; only the background drops it.
            switch phase {
            case .active: model.setActive(true)
            case .background: model.setActive(false)
            default: break
            }
        }
    }
}
