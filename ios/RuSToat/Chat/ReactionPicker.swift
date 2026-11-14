import SwiftUI

/// The reaction picker: the channel's usual choices (or only those the
/// message allows), with search over every emoji. Picking one reacts, or
/// takes the reaction back if the user already has it.
struct ReactionPickerView: View {
    let model: AccountModel
    let channelId: String
    let message: MessageItem
    let pick: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    private var picker: ReactionPicker {
        model.core.reactionPicker(channelId: channelId, messageId: message.id)
    }

    private var mine: Set<String> {
        Set(message.reactions.filter(\.mine).map(\.key))
    }

    var body: some View {
        let picker = picker
        let searching = !query.trimmingCharacters(in: .whitespaces).isEmpty
        let choices = searching
            ? model.core.searchEmoji(channelId: channelId, query: query)
            : picker.choices
        NavigationStack {
            ScrollView {
                if let note = picker.note {
                    Text(note)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal)
                }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 48), spacing: 8)], spacing: 8) {
                    ForEach(choices, id: \.key) { choice in
                        Button {
                            pick(choice.key)
                            dismiss()
                        } label: {
                            EmojiView(emoji: choice.emoji, size: 30)
                                .frame(width: 48, height: 48)
                                .background(mine.contains(choice.key) ? AnyShapeStyle(.tint.opacity(0.2)) : AnyShapeStyle(.clear),
                                            in: .rect(cornerRadius: 10))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding()
                if searching && choices.isEmpty {
                    ContentUnavailableView.search(text: query)
                }
            }
            .navigationTitle("Add Reaction")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close", systemImage: "xmark") { dismiss() }
                }
            }
            .modifier(SearchableUnless(hidden: picker.full || picker.restricted, query: $query))
        }
        .presentationDetents([.medium, .large])
    }
}

/// `.searchable`, unless the choices are limited (a full or restricted message).
private struct SearchableUnless: ViewModifier {
    let hidden: Bool
    @Binding var query: String

    func body(content: Content) -> some View {
        if hidden {
            content
        } else {
            content.searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always),
                               prompt: "Search emoji")
        }
    }
}
