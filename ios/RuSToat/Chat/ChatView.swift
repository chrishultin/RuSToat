import SwiftUI

extension TimelineItem: Identifiable {
    /// The row's identity across updates (a message's key), so SwiftUI
    /// keeps rows in place when older ones are added above.
    public var id: String {
        switch self {
        case .startOfChannel: "start"
        case .newMessages: "new-messages"
        case .message(let message): message.key
        case .blocked(let ids): "blocked-\(ids.first ?? "")"
        }
    }
}

/// A channel's messages, newest at the bottom. Older pages load as the top
/// comes into view; reading to the bottom marks the channel read.
struct ChatView: View {
    let model: AccountModel
    let route: ChannelRoute

    @State private var rows: [TimelineItem] = []
    @State private var loaded = false
    @State private var error: String?
    @State private var loadingOlder = false
    @State private var olderError: String?
    @State private var atBottom = true
    @State private var position = ScrollPosition(edge: .bottom)
    @State private var replyingTo: MessageItem?
    @State private var sendError: String?
    @Environment(\.scenePhase) private var scenePhase

    private var hasMore: Bool {
        if case .startOfChannel = rows.first { return false }
        return true
    }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if loaded && hasMore && !rows.isEmpty {
                    olderSentinel
                }
                ForEach(rows) { row in
                    TimelineRowView(row: row, actions: actions)
                }
            }
            .padding(.vertical, 8)
            .scrollTargetLayout()
        }
        .scrollPosition($position)
        .scrollDismissesKeyboard(.interactively)
        // Start at the bottom, and keep the bottom where it is when the
        // content grows: older rows added above don't move what's on screen.
        .defaultScrollAnchor(.bottom, for: .initialOffset)
        .defaultScrollAnchor(.bottom, for: .sizeChanges)
        .onScrollGeometryChange(for: Bool.self) { geometry in
            geometry.contentOffset.y + geometry.containerSize.height
                >= geometry.contentSize.height - 48
        } action: { _, isAtBottom in
            atBottom = isAtBottom
            markRead()
        }
        .overlay { placeholder }
        .safeAreaBar(edge: .bottom) { bottomBar }
        .navigationTitle(route.name)
        .navigationBarTitleDisplayMode(.inline)
        // The composer takes the bottom; the tabs come back on the way out.
        .toolbar(.hidden, for: .tabBar)
        .environment(\.openURL, OpenURLAction { url in
            // Mentions, channels and the like open in-app later.
            url.scheme == "stoat" ? .handled : .systemAction
        })
        .task(id: route.id) { await open() }
        .onDisappear { model.core.closeChannel(channelId: route.id) }
        .onChange(of: model.timelineVersions[route.id]) { refresh() }
        .onChange(of: model.readyCount) { refresh() }
        .onChange(of: scenePhase) { markRead() }
    }

    @ViewBuilder private var bottomBar: some View {
        if loaded {
            if model.core.canSend(channelId: route.id) || model.channel(route.id) == nil {
                VStack(spacing: 6) {
                    if let sendError {
                        Text(sendError).font(.caption).foregroundStyle(.red)
                    }
                    Composer(
                        text: draft,
                        replyingTo: $replyingTo,
                        placeholder: placeholderText,
                        onChange: { text in
                            model.core.typing(channelId: route.id, empty: text.isEmpty)
                        },
                        onSend: send
                    )
                }
            } else {
                CantSendBar()
            }
        }
    }

    private var draft: Binding<String> {
        Binding(
            get: { model.drafts[route.id] ?? "" },
            set: { model.drafts[route.id] = $0 }
        )
    }

    private var placeholderText: String {
        switch model.channel(route.id)?.kind {
        case .text, .voice: "Message #\(route.name)"
        case .savedNotes: "Save a note"
        default: "Message \(route.name)"
        }
    }

    private var actions: MessageActions {
        MessageActions(
            reply: { message in replyingTo = message },
            retry: { key in model.core.retry(channelId: route.id, key: key) },
            discard: { key in model.core.discard(channelId: route.id, key: key) }
        )
    }

    private func send() {
        let text = model.drafts[route.id] ?? ""
        do {
            _ = try model.core.send(channelId: route.id, text: text, replyTo: replyingTo?.id)
            model.drafts[route.id] = nil
            replyingTo = nil
            sendError = nil
            withAnimation { position.scrollTo(edge: .bottom) }
        } catch let CoreError.Failed(message) {
            sendError = message
        } catch {
            sendError = "Couldn't send that."
        }
    }

    @ViewBuilder private var placeholder: some View {
        if !loaded {
            if let error {
                ContentUnavailableView {
                    Label("Couldn't load messages", systemImage: "exclamationmark.bubble")
                } description: {
                    Text(error)
                } actions: {
                    Button("Retry") { Task { await open() } }
                        .buttonStyle(.glassProminent)
                }
            } else {
                ProgressView()
            }
        }
    }

    private var olderSentinel: some View {
        Group {
            if let olderError {
                Button("\(olderError) Retry", systemImage: "arrow.clockwise") {
                    self.olderError = nil
                    loadOlder()
                }
                .font(.footnote)
                .buttonStyle(.glass)
            } else {
                ProgressView()
                    .onAppear(perform: loadOlder)
            }
        }
        .frame(maxWidth: .infinity)
        .padding()
    }

    private func open() async {
        error = nil
        do {
            try await model.core.openChannel(channelId: route.id)
            loaded = true
            refresh()
        } catch let CoreError.Failed(message) {
            error = message
        } catch {
            self.error = "Something went wrong loading messages."
        }
    }

    private func refresh() {
        rows = model.core.timeline(channelId: route.id)
        if rows.isEmpty && loaded {
            // Dropped by a reconnect; the core reloads it.
            return
        }
        markRead()
    }

    private func loadOlder() {
        guard !loadingOlder, hasMore else { return }
        loadingOlder = true
        Task {
            defer { loadingOlder = false }
            do {
                _ = try await model.core.loadOlder(channelId: route.id)
            } catch let CoreError.Failed(message) {
                olderError = message
            } catch {
                olderError = "Couldn't load older messages."
            }
        }
    }

    /// Only while the newest message is in view, with the app in front.
    private func markRead() {
        guard atBottom, loaded, scenePhase == .active else { return }
        _ = model.core.markRead(channelId: route.id)
    }
}

/// One row: a message, the divider, the start of the channel, or hidden
/// blocked messages.
struct TimelineRowView: View {
    let row: TimelineItem
    let actions: MessageActions

    var body: some View {
        switch row {
        case .startOfChannel(let name):
            VStack(alignment: .leading, spacing: 4) {
                Text("Welcome to \(name)")
                    .font(.title3.bold())
                Text("This is the start of the conversation.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 20)
        case .newMessages:
            HStack(spacing: 8) {
                Rectangle().fill(.red).frame(height: 1)
                Text("New messages")
                    .font(.caption.bold())
                    .foregroundStyle(.red)
                Rectangle().fill(.red).frame(height: 1)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
        case .message(let message):
            MessageRowView(message: message, actions: actions)
        case .blocked(let ids):
            Text(ids.count == 1 ? "1 blocked message" : "\(ids.count) blocked messages")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
        }
    }
}
