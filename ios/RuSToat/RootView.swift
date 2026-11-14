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
            LoginView(core: model.core) { model.loggedIn($0) }
        case .signedIn(let account):
            SignedInTabs(account: account, model: model)
        }
    }
}

/// The app's tabs. Home and Servers are placeholders until their chunks land.
private struct SignedInTabs: View {
    let account: Account
    let model: AppModel

    var body: some View {
        TabView {
            Tab("Home", systemImage: "bubble.left.and.bubble.right") {
                PlaceholderList(title: "Home")
            }
            Tab("Servers", systemImage: "square.stack.3d.up") {
                PlaceholderList(title: "Servers")
            }
            Tab("You", systemImage: "person.crop.circle") {
                NavigationStack {
                    List {
                        LabeledContent("Signed in as", value: account.name)
                        LabeledContent("Server", value: account.instance)
                        Button("Log Out", role: .destructive) {
                            Task { await model.logOut() }
                        }
                    }
                    .navigationTitle("You")
                }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
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

/// A scrolling list, so the tab bar's minimise-on-scroll can be seen.
private struct PlaceholderList: View {
    let title: String

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Core", value: coreVersion())
                }
                Section {
                    ForEach(1...40, id: \.self) { row in
                        Text("Row \(row)")
                    }
                }
            }
            .navigationTitle(title)
        }
    }
}
