import SwiftUI

/// The app's tabs. Each is a placeholder until its milestone chunk lands.
struct RootView: View {
    var body: some View {
        TabView {
            Tab("Home", systemImage: "bubble.left.and.bubble.right") {
                PlaceholderList(title: "Home")
            }
            Tab("Servers", systemImage: "square.stack.3d.up") {
                PlaceholderList(title: "Servers")
            }
            Tab("You", systemImage: "person.crop.circle") {
                PlaceholderList(title: "You")
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
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

#Preview {
    RootView()
}
