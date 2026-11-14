import SwiftUI
import UIKit

/// The files attached to the message being typed, above the box: each with
/// a preview (images) or icon, its name and size, a spoiler toggle, and a
/// button to take it off.
struct AttachmentTray: View {
    @Binding var files: [TrayFile]
    let remove: (TrayFile) -> Void

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                ForEach($files) { $file in
                    TrayChip(file: $file) { remove(file) }
                }
            }
            .padding(.horizontal, 4)
        }
        .scrollIndicators(.hidden)
    }
}

private struct TrayChip: View {
    @Binding var file: TrayFile
    let remove: () -> Void
    @State private var preview: UIImage?
    @Environment(\.displayScale) private var scale

    var body: some View {
        HStack(spacing: 8) {
            Group {
                if let preview {
                    Image(uiImage: preview)
                        .resizable()
                        .scaledToFill()
                        .blur(radius: file.spoiler ? 6 : 0)
                } else {
                    Image(systemName: file.isImage ? "photo" : "doc")
                        .foregroundStyle(.tint)
                }
            }
            .frame(width: 36, height: 36)
            .background(.quaternary.opacity(0.5))
            .clipShape(.rect(cornerRadius: 6))
            VStack(alignment: .leading, spacing: 1) {
                Text(file.name)
                    .font(.caption.weight(.semibold))
                    .lineLimit(1)
                Text(file.spoiler ? "\(file.sizeLabel) · Spoiler" : file.sizeLabel)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: 140, alignment: .leading)
            Button(file.spoiler ? "Unmark Spoiler" : "Mark as Spoiler",
                   systemImage: file.spoiler ? "eye.slash.fill" : "eye") {
                file.spoiler.toggle()
            }
            .labelStyle(.iconOnly)
            .font(.callout)
            .foregroundStyle(file.spoiler ? Color.accentColor : .secondary)
            Button("Remove", systemImage: "xmark.circle.fill", action: remove)
                .labelStyle(.iconOnly)
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        .padding(.leading, 6)
        .padding(.trailing, 10)
        .padding(.vertical, 6)
        .glassEffect(.regular, in: .rect(cornerRadius: 14))
        .task(id: file.url) {
            guard file.isImage else { return }
            let url = file.url
            let pixels = ImageLoader.pixels(fitting: CGSize(width: 36, height: 36), scale: scale)
            preview = await Task.detached(priority: .userInitiated) { [scale] in
                (try? Data(contentsOf: url)).flatMap { ImageLoader.decode($0, maxPixels: pixels, scale: scale) }
            }.value
        }
    }
}

/// A file of a message being sent: a picture from the local copy at the
/// size the sent one will have (covered if it's a spoiler), or a card, with
/// how much has uploaded.
struct LocalAttachmentView: View {
    let local: LocalAttachment
    @Environment(\.mediaMaxWidth) private var available
    @Environment(\.displayScale) private var scale
    @State private var picture: UIImage?

    var body: some View {
        if let path = local.imagePath {
            let size = MediaLayout.fit(width: local.width, height: local.height, available: available)
            ZStack {
                if let picture {
                    Image(uiImage: picture)
                        .resizable()
                        .scaledToFill()
                        .blur(radius: local.spoiler ? 24 : 0, opaque: true)
                } else {
                    Rectangle().fill(.quaternary)
                }
                Color.black.opacity(local.progress < 1000 ? 0.3 : 0)
                // On a dark disc, to show on any picture.
                UploadRing(progress: local.progress)
                    .padding(8)
                    .background(.black.opacity(0.5), in: .circle)
            }
            .frame(width: size.width, height: size.height)
            .clipShape(.rect(cornerRadius: 12))
            .task(id: path) {
                let pixels = ImageLoader.pixels(fitting: size, scale: scale)
                picture = await Task.detached(priority: .userInitiated) { [scale] in
                    (try? Data(contentsOf: URL(filePath: path)))
                        .flatMap { ImageLoader.decode($0, maxPixels: pixels, scale: scale) }
                }.value
            }
        } else {
            HStack(spacing: 10) {
                UploadRing(progress: local.progress, tint: .accentColor)
                VStack(alignment: .leading) {
                    Text(local.name).lineLimit(1)
                    Text(local.spoiler ? "\(local.sizeLabel) · Spoiler" : local.sizeLabel)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(10)
            .frame(maxWidth: 280, alignment: .leading)
            .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 12))
        }
    }
}

/// How much of a file has uploaded (thousandths), as a ring; a tick once
/// it's all there.
struct UploadRing: View {
    let progress: UInt16
    var tint: Color = .white

    var body: some View {
        let fraction = Double(min(progress, 1000)) / 1000
        ZStack {
            Circle().stroke(tint.opacity(0.3), lineWidth: 3)
            Circle()
                .trim(from: 0, to: fraction)
                .stroke(tint, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .animation(.linear(duration: 0.1), value: fraction)
            if progress >= 1000 {
                Image(systemName: "checkmark")
                    .font(.caption.bold())
                    .foregroundStyle(tint)
            }
        }
        .frame(width: 28, height: 28)
        .accessibilityLabel("Uploaded \(Int(fraction * 100)) percent")
    }
}
