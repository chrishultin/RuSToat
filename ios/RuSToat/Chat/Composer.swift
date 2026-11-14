import SwiftUI

/// The message box: a growing text field and a send button floating in
/// Liquid Glass, with what's being replied to above it.
struct Composer: View {
    @Binding var text: String
    @Binding var replyingTo: MessageItem?
    /// Editing one of the user's messages: the box holds its text, and
    /// sending saves it.
    var editing = false
    var onCancelEdit: () -> Void = {}
    let placeholder: String
    let onChange: (String) -> Void
    let onSend: () -> Void
    @FocusState private var focused: Bool

    private var empty: Bool {
        text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        GlassEffectContainer(spacing: 8) {
            VStack(alignment: .leading, spacing: 8) {
                if editing {
                    HStack(spacing: 6) {
                        Image(systemName: "pencil")
                            .font(.caption)
                        Text("Editing message")
                            .font(.caption)
                        Button("Cancel editing", systemImage: "xmark.circle.fill", action: onCancelEdit)
                            .labelStyle(.iconOnly)
                            .font(.caption)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .glassEffect(.regular, in: .capsule)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                } else if let reply = replyingTo {
                    HStack(spacing: 6) {
                        Image(systemName: "arrowshape.turn.up.left.fill")
                            .font(.caption)
                        Text("Replying to \(Text(reply.author).bold())")
                            .font(.caption)
                            .lineLimit(1)
                        Button("Cancel reply", systemImage: "xmark.circle.fill") {
                            replyingTo = nil
                        }
                        .labelStyle(.iconOnly)
                        .font(.caption)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .glassEffect(.regular, in: .capsule)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                }
                HStack(alignment: .bottom, spacing: 8) {
                    TextField(placeholder, text: $text, axis: .vertical)
                        .lineLimit(1...6)
                        .focused($focused)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 11)
                        .glassEffect(.regular.interactive(), in: .rect(cornerRadius: 22))
                        .onChange(of: text) { _, new in onChange(new) }
                    Button(action: send) {
                        Image(systemName: editing ? "checkmark" : "arrow.up")
                            .font(.body.weight(.semibold))
                            .frame(width: 28, height: 28)
                    }
                    .buttonStyle(.glassProminent)
                    .buttonBorderShape(.circle)
                    .disabled(empty && !editing)
                    .accessibilityLabel(editing ? "Save" : "Send")
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 8)
        .animation(.default, value: replyingTo?.key)
        .animation(.default, value: editing)
        .onChange(of: replyingTo?.key) { _, key in
            if key != nil { focused = true }
        }
        .onChange(of: editing) { _, editing in
            if editing { focused = true }
        }
    }

    private func send() {
        // An empty edit is refused by the core, with a hint to use Delete.
        guard !empty || editing else { return }
        onSend()
    }
}

/// Instead of the message box, where the user can't send.
struct CantSendBar: View {
    var body: some View {
        Label("You can't send messages here.", systemImage: "lock.fill")
            .font(.footnote)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .glassEffect(.regular, in: .capsule)
            .padding(.bottom, 8)
    }
}
