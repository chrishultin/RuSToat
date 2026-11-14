import SwiftUI

/// A glass capsule at the top saying the connection isn't up, if it isn't.
struct ConnectionBanner: View {
    let status: ConnectionStatus

    var body: some View {
        if let text {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(text)
                    .font(.footnote.weight(.medium))
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .glassEffect(.regular, in: .capsule)
            .transition(.move(edge: .top).combined(with: .opacity))
            .accessibilityElement(children: .combine)
        }
    }

    private var text: String? {
        switch status {
        case .connecting(let attempt):
            attempt <= 1 ? "Connecting…" : "Reconnecting (attempt \(attempt))…"
        case .disconnected(_, let retryInSeconds):
            retryInSeconds > 1 ? "Offline. Retrying in \(retryInSeconds)s…" : "Offline. Retrying…"
        case .connected, .sessionRejected, .paused:
            nil
        }
    }
}
