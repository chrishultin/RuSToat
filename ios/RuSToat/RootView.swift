import SwiftUI

/// Login, or the app's tabs once signed in.
struct RootView: View {
    let model: AppModel

    var body: some View {
        switch model.phase {
        case .restoring:
            ProgressView("Signing in…")
        case .offline(let account, let message):
            OfflineView(account: account, message: message, model: model)
        case .signedOut:
            LoginView(core: model.core, notice: model.loginNotice) { model.loggedIn($0) }
        case .signedIn(let account):
            SignedInTabs(account: account, model: model)
        }
    }
}

/// The app's tabs, with the connection banner over them. The tab (and
/// each tab's open channel) is remembered across launches.
private struct SignedInTabs: View {
    let account: Account
    let model: AppModel
    @SceneStorage("tab") private var tab = "home"

    var body: some View {
        if let accountModel = model.accountModel {
            TabView(selection: $tab) {
                Tab("Home", systemImage: "bubble.left.and.bubble.right", value: "home") {
                    HomeView(model: accountModel)
                }
                .badge(Int(accountModel.summary.homeMentions))
                Tab("Servers", systemImage: "square.stack.3d.up", value: "servers") {
                    ServerListView(model: accountModel)
                }
                .badge(Int(accountModel.summary.serversMentions))
                Tab("You", systemImage: "person.crop.circle", value: "you") {
                    YouView(account: account, accountModel: accountModel) {
                        await model.logOut()
                    }
                }
            }
            .tabBarMinimizeBehavior(.onScrollDown)
            .overlay(alignment: .top) {
                ConnectionBanner(status: accountModel.status)
                    .padding(.top, 4)
                    .animation(.default, value: accountModel.status)
            }
        }
    }
}

/// The saved session couldn't be checked: offer to try again.
private struct OfflineView: View {
    let account: Account
    let message: String
    let model: AppModel

    var body: some View {
        ContentUnavailableView {
            Label("Can't reach Stoat", systemImage: "wifi.exclamationmark")
        } description: {
            Text(message)
        } actions: {
            Button("Retry") { Task { await model.restore() } }
                .buttonStyle(.glassProminent)
            Button("Log in again") { model.showLogin() }
                .buttonStyle(.glass)
        }
    }
}
