import ImageIO
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// A picture the full-screen viewer can show.
struct ViewerImage: Identifiable, Hashable {
    /// A file's ID, or an embed's address.
    let id: String
    let url: URL
    let name: String
    let thumbhash: Data?
    /// Its shape, as the row drew it: the inline copy to show while the
    /// full picture loads is cached at this size.
    let inlineSize: CGSize

    /// The pictures of `message` that are showing in its row (not too big
    /// to load, nor covered), in order: attachments, then embeds.
    static func images(of message: MessageItem, available: CGFloat) -> [ViewerImage] {
        var images: [ViewerImage] = []
        func add(_ attachment: AttachmentItem) {
            guard case .image(let image) = attachment, !image.tooBig, !image.spoiler,
                  let url = URL(string: image.url) else { return }
            images.append(ViewerImage(
                id: image.id, url: url, name: image.name, thumbhash: image.thumbhash,
                inlineSize: MediaLayout.fit(width: image.width, height: image.height, available: available)
            ))
        }
        message.attachments.forEach(add)
        for embed in message.embeds {
            switch embed {
            case .image(let image):
                guard let url = URL(string: image.url) else { continue }
                let name = URL(string: image.source)?.lastPathComponent ?? "image"
                images.append(ViewerImage(
                    id: image.url, url: url, name: name.isEmpty ? "image" : name, thumbhash: nil,
                    inlineSize: MediaLayout.fit(width: image.width, height: image.height, available: available)
                ))
            case .card(let card):
                if let media = card.media { add(media) }
            case .video, .gifPoster:
                break
            }
        }
        return images
    }
}

/// What the viewer opens on.
struct ViewerRequest: Identifiable {
    let id = UUID()
    let images: [ViewerImage]
    let start: Int

    /// `nil` when `id` isn't one of `images`.
    init?(images: [ViewerImage], showing id: String) {
        guard let start = images.firstIndex(where: { $0.id == id }) else { return nil }
        self.images = images
        self.start = start
    }
}

/// The viewer's arithmetic, kept apart for testing.
enum ViewerMath {
    static let maxZoom: CGFloat = 4
    /// A double tap zooms to this, or back out.
    static let doubleTapZoom: CGFloat = 2.5
    /// A drag down this far, or flung this fast (points a second), closes.
    static let dismissDistance: CGFloat = 120
    static let dismissSpeed: CGFloat = 900

    /// The page after paging by `delta` from `index`, among `count`.
    static func page(_ index: Int, by delta: Int, count: Int) -> Int {
        guard count > 0 else { return 0 }
        return min(max(index + delta, 0), count - 1)
    }

    static func clampZoom(_ scale: CGFloat) -> CGFloat {
        min(max(scale, 1), maxZoom)
    }

    /// Where a double tap goes from `scale`: in to `doubleTapZoom`, or back
    /// to the whole picture when zoomed at all.
    static func doubleTapTarget(from scale: CGFloat) -> CGFloat {
        scale > 1.01 ? 1 : doubleTapZoom
    }

    /// A picture of `aspect` fitted inside `bounds`, keeping its shape.
    static func fitted(_ aspect: CGSize, in bounds: CGSize) -> CGSize {
        guard aspect.width > 0, aspect.height > 0, bounds.width > 0, bounds.height > 0 else { return bounds }
        let scale = min(bounds.width / aspect.width, bounds.height / aspect.height)
        return CGSize(width: aspect.width * scale, height: aspect.height * scale)
    }

    /// The rectangle (in the unzoomed picture's points) to zoom into so that
    /// `point` is centred at `scale`, for a view of `bounds`.
    static func zoomRect(around point: CGPoint, scale: CGFloat, bounds: CGSize) -> CGRect {
        let size = CGSize(width: bounds.width / scale, height: bounds.height / scale)
        return CGRect(x: point.x - size.width / 2, y: point.y - size.height / 2,
                      width: size.width, height: size.height)
    }

    /// Whether a drag down that ended at `translation` with `velocity`
    /// closes the viewer.
    static func dismisses(translation: CGFloat, velocity: CGFloat) -> Bool {
        translation > dismissDistance || (translation > 20 && velocity > dismissSpeed)
    }
}

/// Every picture of a message, full screen: swipe between them, pinch or
/// double-tap to zoom, drag down to close, and share the file.
struct ImageViewer: View {
    let request: ViewerRequest
    var loader: ImageLoader = .shared

    @Environment(\.dismiss) private var dismiss
    @State private var index = 0
    @State private var zoomed = false
    @State private var drag: CGFloat = 0
    @State private var chrome = true
    @State private var shared: [String: URL] = [:]

    private var current: ViewerImage { request.images[index] }

    var body: some View {
        TabView(selection: $index) {
            ForEach(Array(request.images.enumerated()), id: \.element.id) { offset, image in
                ViewerPage(
                    image: image,
                    loader: loader,
                    onZoom: { zoomed = $0 },
                    onTap: { withAnimation(.easeOut(duration: 0.2)) { chrome.toggle() } },
                    onDrag: dragged
                )
                .tag(offset)
            }
        }
        .tabViewStyle(.page(indexDisplayMode: .never))
        .offset(y: drag)
        .background(.black.opacity(1 - min(Double(drag) / 400, 0.7)))
        .ignoresSafeArea()
        .overlay(alignment: .top) {
            if chrome && drag == 0 { topBar }
        }
        .statusBarHidden(!chrome)
        .presentationBackground(.clear)
        .onAppear { index = request.start }
        .task(id: index) { await prepareShare(current) }
        .onDisappear(perform: ViewerFiles.removeAll)
    }

    private func dragged(_ phase: ViewerPage.DragPhase) {
        switch phase {
        case .changed(let y):
            drag = max(y, 0)
        case .ended(let y, let velocity):
            if ViewerMath.dismisses(translation: y, velocity: velocity) {
                dismiss()
            } else {
                withAnimation(.spring(duration: 0.3)) { drag = 0 }
            }
        }
    }

    private var topBar: some View {
        HStack(spacing: 12) {
            Button("Close", systemImage: "xmark") { dismiss() }
                .labelStyle(.iconOnly)
                .buttonStyle(.glass)
                .buttonBorderShape(.circle)
            VStack(spacing: 0) {
                Text(current.name)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                if request.images.count > 1 {
                    Text("\(index + 1) of \(request.images.count)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity)
            .foregroundStyle(.white)
            if let file = shared[current.id] {
                ShareLink(item: file) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.glass)
                .buttonBorderShape(.circle)
            } else {
                Button("Share", systemImage: "square.and.arrow.up") {}
                    .labelStyle(.iconOnly)
                    .buttonStyle(.glass)
                    .buttonBorderShape(.circle)
                    .disabled(true)
            }
        }
        .padding(.horizontal, 16)
        .transition(.opacity)
    }

    /// The share sheet shares a file, named as the attachment is, so it
    /// shows the right name and type.
    private func prepareShare(_ image: ViewerImage) async {
        guard shared[image.id] == nil, let data = await loader.data(image.url) else { return }
        shared[image.id] = ViewerFiles.write(data, named: image.name, id: image.id)
    }
}

/// Pictures written out for the share sheet, in the temporary folder until
/// the viewer closes.
enum ViewerFiles {
    static var folder: URL {
        FileManager.default.temporaryDirectory.appending(path: "viewer", directoryHint: .isDirectory)
    }

    /// `data` as a file called `name` (given an extension for its type if it
    /// has none), in a folder of its own.
    static func write(_ data: Data, named name: String, id: String) -> URL? {
        let safeID = id.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? UUID().uuidString
        let directory = folder.appending(path: String(safeID.prefix(80)), directoryHint: .isDirectory)
        let file = directory.appending(path: fileName(name, for: data))
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try data.write(to: file, options: .atomic)
            return file
        } catch {
            return nil
        }
    }

    /// `name` without path separators, with an extension for the picture's
    /// type when it has none (proxied embeds often don't).
    static func fileName(_ name: String, for data: Data) -> String {
        var name = name.replacingOccurrences(of: "/", with: "_")
        if name.isEmpty { name = "image" }
        guard (name as NSString).pathExtension.isEmpty else { return name }
        let source = CGImageSourceCreateWithData(data as CFData, nil)
        let type = source.flatMap(CGImageSourceGetType).flatMap { UTType($0 as String) }
        return type?.preferredFilenameExtension.map { "\(name).\($0)" } ?? name
    }

    static func removeAll() {
        try? FileManager.default.removeItem(at: folder)
    }
}

/// One picture in the viewer: the inline copy (or its preview) at once,
/// then the full picture, at up to three times the screen's size.
struct ViewerPage: View {
    enum DragPhase {
        case changed(CGFloat)
        case ended(CGFloat, velocity: CGFloat)
    }

    let image: ViewerImage
    let loader: ImageLoader
    let onZoom: (Bool) -> Void
    let onTap: () -> Void
    let onDrag: (DragPhase) -> Void

    @Environment(\.displayScale) private var scale
    @State private var full: UIImage?

    var body: some View {
        GeometryReader { geometry in
            ZoomableImage(
                image: full ?? placeholder,
                aspect: image.inlineSize,
                onZoom: onZoom,
                onTap: onTap,
                onDrag: onDrag
            )
            .task(id: image.id) {
                let screen = CGSize(width: max(geometry.size.width, 1), height: max(geometry.size.height, 1))
                full = await loader.image(image.url, fitting: screen, scale: 3)
            }
        }
        .ignoresSafeArea()
    }

    private var placeholder: UIImage? {
        loader.cached(image.url, fitting: image.inlineSize, scale: scale)
            ?? image.thumbhash.flatMap(ThumbHashes.image)
    }
}

/// A picture in a `UIScrollView`, which pans a zoomed picture far better
/// than SwiftUI's gestures: pinch and double-tap zoom, a tap, and (at full
/// size) a vertical drag reported for closing.
struct ZoomableImage: UIViewRepresentable {
    let image: UIImage?
    /// The picture's shape, so a preview of slightly different proportions
    /// is laid out as the picture will be.
    let aspect: CGSize
    let onZoom: (Bool) -> Void
    let onTap: () -> Void
    let onDrag: (ViewerPage.DragPhase) -> Void

    func makeUIView(context: Context) -> ZoomView {
        ZoomView()
    }

    func updateUIView(_ view: ZoomView, context: Context) {
        view.onZoom = onZoom
        view.onTap = onTap
        view.onDrag = onDrag
        view.show(image, aspect: aspect)
    }
}

final class ZoomView: UIScrollView, UIScrollViewDelegate, UIGestureRecognizerDelegate {
    private let imageView = UIImageView()
    private var aspect = CGSize(width: 1, height: 1)
    private var laidOutFor: CGSize = .zero
    private var wasZoomed = false
    private lazy var dismissPan = UIPanGestureRecognizer(target: self, action: #selector(panned))
    var onZoom: (Bool) -> Void = { _ in }
    var onTap: () -> Void = {}
    var onDrag: (ViewerPage.DragPhase) -> Void = { _ in }

    init() {
        super.init(frame: .zero)
        delegate = self
        minimumZoomScale = 1
        maximumZoomScale = ViewerMath.maxZoom
        showsVerticalScrollIndicator = false
        showsHorizontalScrollIndicator = false
        contentInsetAdjustmentBehavior = .never
        decelerationRate = .fast
        backgroundColor = .clear
        imageView.contentMode = .scaleAspectFit
        imageView.clipsToBounds = true
        addSubview(imageView)

        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(doubleTapped))
        doubleTap.numberOfTapsRequired = 2
        addGestureRecognizer(doubleTap)
        let tap = UITapGestureRecognizer(target: self, action: #selector(tapped))
        tap.require(toFail: doubleTap)
        addGestureRecognizer(tap)
        dismissPan.delegate = self
        addGestureRecognizer(dismissPan)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(_ image: UIImage?, aspect: CGSize) {
        if imageView.image !== image {
            imageView.image = image
        }
        if self.aspect != aspect {
            self.aspect = aspect
            laidOutFor = .zero
            setNeedsLayout()
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if laidOutFor != bounds.size {
            laidOutFor = bounds.size
            zoomScale = 1
            let size = ViewerMath.fitted(aspect, in: bounds.size)
            imageView.frame = CGRect(origin: .zero, size: size)
            contentSize = size
        }
        centre()
    }

    /// Keep a picture smaller than the view in its middle.
    private func centre() {
        let x = max((bounds.width - contentSize.width) / 2, 0)
        let y = max((bounds.height - contentSize.height) / 2, 0)
        contentInset = UIEdgeInsets(top: y, left: x, bottom: y, right: x)
    }

    func viewForZooming(in scrollView: UIScrollView) -> UIView? { imageView }

    func scrollViewDidZoom(_ scrollView: UIScrollView) {
        centre()
        let zoomed = zoomScale > 1.01
        if zoomed != wasZoomed {
            wasZoomed = zoomed
            onZoom(zoomed)
        }
    }

    @objc private func doubleTapped(_ gesture: UITapGestureRecognizer) {
        let target = ViewerMath.doubleTapTarget(from: zoomScale)
        if target == 1 {
            setZoomScale(1, animated: true)
        } else {
            let point = gesture.location(in: imageView)
            zoom(to: ViewerMath.zoomRect(around: point, scale: target, bounds: bounds.size), animated: true)
        }
    }

    @objc private func tapped() { onTap() }

    @objc private func panned(_ gesture: UIPanGestureRecognizer) {
        let y = gesture.translation(in: self).y
        switch gesture.state {
        case .changed:
            onDrag(.changed(y))
        case .ended, .cancelled, .failed:
            onDrag(.ended(y, velocity: gesture.velocity(in: self).y))
        default:
            break
        }
    }

    // Drag-to-close starts only at full size and mostly downwards, so
    // sideways drags still page and zoomed drags still pan.
    override func gestureRecognizerShouldBegin(_ gesture: UIGestureRecognizer) -> Bool {
        guard gesture === dismissPan else { return super.gestureRecognizerShouldBegin(gesture) }
        let velocity = dismissPan.velocity(in: self)
        return zoomScale <= 1.01 && velocity.y > 0 && abs(velocity.y) > abs(velocity.x)
    }

    // Other pans (the pager's, and this view's own scrolling) wait to see
    // whether this is a drag down.
    func gestureRecognizer(
        _ gesture: UIGestureRecognizer,
        shouldBeRequiredToFailBy other: UIGestureRecognizer
    ) -> Bool {
        gesture === dismissPan && other is UIPanGestureRecognizer && other !== dismissPan
    }
}
