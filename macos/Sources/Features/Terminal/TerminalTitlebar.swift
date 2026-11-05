import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// Native frames keep the app button fixed while the repository follows the divider.
final class TerminalTitlebarContainer: NSView {
    let contextView: NonDraggableHostingView<TerminalTitlebar>
    let applicationView: NonDraggableHostingView<TerminalOpenWithButton>
    private let divider = TabSidebarResizeView()
    private var preferredWidth: CGFloat = 0
    private var sidebarOffset: CGFloat = 0
    private var sidebarVisible = false
    private var contextObservation: AnyCancellable?

    init(model: TerminalTabModel) {
        contextView = NonDraggableHostingView(rootView: TerminalTitlebar(model: model))
        applicationView = NonDraggableHostingView(rootView: TerminalOpenWithButton(model: model))
        super.init(frame: NSRect(x: 0, y: 0, width: 0, height: 32))
        contextView.sizingOptions = []
        applicationView.sizingOptions = []
        contextView.wantsLayer = true
        contextView.layer?.masksToBounds = true
        addSubview(contextView)
        addSubview(applicationView)
        addSubview(divider)
        setAccessibilityElement(false)
        setAccessibilityIdentifier("Terminal Title Bar")
        contextObservation = model.$directoryContext.sink { [weak self] context in
            self?.toolTip = context?.gitStatus?.detail ?? context?.directory.path
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var intrinsicContentSize: NSSize { NSSize(width: preferredWidth, height: 32) }
    override var mouseDownCanMoveWindow: Bool { true }
    override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard let target = super.hitTest(point) else { return nil }
        return target.isDescendant(of: contextView) ? self : target
    }

    func updateGeometry(width: CGFloat, leading: CGFloat, sidebarWidth: CGFloat, sidebarVisible: Bool) {
        if preferredWidth != width {
            preferredWidth = width
            setFrameSize(NSSize(width: width, height: 32))
            invalidateIntrinsicContentSize()
        }
        self.sidebarVisible = sidebarVisible
        sidebarOffset = sidebarVisible ? max(0, sidebarWidth - leading) : 0
        needsLayout = true
        layoutSubtreeIfNeeded()
    }

    override func layout() {
        super.layout()
        let applicationX = max(0, bounds.width - 64)
        let contextX = sidebarOffset + 12
        let contextWidth = max(0, applicationX - contextX - 12)
        contextView.frame = NSRect(x: contextX, y: 0, width: contextWidth, height: bounds.height)
        let applicationFrame = NSRect(x: applicationX, y: (bounds.height - 24) / 2, width: 52, height: 24)
        if applicationView.frame != applicationFrame { applicationView.frame = applicationFrame }
        let hasRoom = applicationX - sidebarOffset >= 24
        contextView.isHidden = !hasRoom
        applicationView.isHidden = !hasRoom
        divider.isHidden = !sidebarVisible
        divider.frame = NSRect(x: sidebarOffset - 6, y: 0, width: 6, height: bounds.height)
    }
}

struct TerminalDirectoryContext: Equatable {
    let directory: URL
    let gitStatus: GitTabStatus?

    var branch: String? { gitStatus?.titlebarBranch }

    var name: String { directory.lastPathComponent.isEmpty ? directory.path : directory.lastPathComponent }

    init?(workingDirectory: URL?, gitStatus: GitTabStatus?) {
        guard let workingDirectory else { return nil }
        directory = gitStatus?.directory ?? workingDirectory
        self.gitStatus = gitStatus
    }
}

struct TerminalTitlebar: View {
    @ObservedObject var model: TerminalTabModel

    var body: some View {
        HStack(spacing: 8) {
            HStack(spacing: 8) {
                if let url = model.directoryContext?.gitStatus?.ownerAvatarURL {
                    AsyncImage(url: url) { phase in
                        if let image = phase.image {
                            image.resizable().scaledToFill()
                        } else {
                            Image(systemName: "folder").foregroundStyle(.secondary)
                        }
                    }
                        .frame(width: 16, height: 16)
                        .clipShape(RoundedRectangle(cornerRadius: 3))
                        .id(url)
                        .accessibilityHidden(true)
                }
                Text(model.directoryContext?.name ?? "Terminal")
                    .truncationMode(.middle)
                if let status = model.directoryContext?.gitStatus {
                    TerminalTitlebarGitIndicator(status: status)
                        .accessibilityHidden(true)
                }
                if let branch = model.directoryContext?.branch {
                    Text("[\(branch)]")
                        .foregroundStyle(.secondary)
                        .truncationMode(.middle)
                }
                if let changes = model.directoryContext?.gitStatus?.lineChanges,
                   changes.additions != 0 || changes.deletions != 0 {
                    HStack(spacing: 6) {
                        Text("+\(changes.additions)").foregroundStyle(.green)
                        Text("-\(changes.deletions)").foregroundStyle(.red)
                    }
                    .monospacedDigit()
                    .fixedSize()
                    .accessibilityLabel("\(changes.additions) lines added, \(changes.deletions) lines deleted")
                }
            }
            .font(.system(size: 12))
            .lineLimit(1)
            .help(model.directoryContext?.gitStatus?.detail
                  ?? model.directoryContext?.directory.path ?? "Working directory unavailable")
            .accessibilityElement(children: .combine)
            .accessibilityLabel(model.directoryContext?.gitStatus?.detail
                                 ?? model.directoryContext?.name ?? "Terminal")
            .allowsHitTesting(false)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .transaction { $0.disablesAnimations = true }
        .clipped()
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("Terminal Title Bar")
    }
}

private struct TerminalTitlebarGitIndicator: View {
    let status: GitTabStatus

    var body: some View {
        Group {
            if status.indicator == .draft {
                DraftPullRequestIcon()
                    .fill(style: FillStyle(eoFill: true))
            } else if let name = status.indicator.assetName {
                Image(name).resizable()
            } else {
                Image("GitBranch").resizable()
            }
        }
        .foregroundStyle(iconColor)
        .frame(width: 16, height: 16)
    }

    private var iconColor: Color {
        switch status.indicator {
        case .standard: .secondary
        case .open: .green
        case .draft: .primary
        case .merged: .purple
        case .conflict: .red
        }
    }
}

private struct DraftPullRequestIcon: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        for center in [CGPoint(x: 4, y: 2), CGPoint(x: 4, y: 14), CGPoint(x: 12, y: 14)] {
            path.addEllipse(in: CGRect(x: center.x - 2, y: center.y - 2, width: 4, height: 4))
            path.addEllipse(in: CGRect(x: center.x - 0.75, y: center.y - 0.75, width: 1.5, height: 1.5))
        }
        path.addRect(CGRect(x: 3.25, y: 4, width: 1.5, height: 8))
        for y in [2.0, 6.0, 10.0] {
            path.addEllipse(in: CGRect(x: 11, y: y - 1, width: 2, height: 2))
        }
        return path.applying(CGAffineTransform(a: rect.width / 16, b: 0, c: 0, d: rect.height / 16,
                                              tx: rect.minX, ty: rect.minY))
    }
}

struct TerminalOpenWithButton: View {
    @ObservedObject var model: TerminalTabModel
    @AppStorage("TerminalOpenWithApplication", store: UserDefaults.ghostty)
    private var selectedID = ""
    @AppStorage("TerminalOpenWithApplicationPath", store: UserDefaults.ghostty)
    private var selectedPath = ""
    @State private var applications: [TerminalExternalApplication] = []
    @State private var error: String?

    var body: some View {
        let selected = applications.first { $0.url.path == selectedPath }
            ?? applications.first { $0.id == selectedID } ?? applications.first
        HStack(spacing: 0) {
            Button {
                if let selected { open(selected) }
            } label: {
                Group {
                    if let selected {
                        Image(nsImage: selected.icon).resizable()
                    } else {
                        Image(systemName: "app").resizable()
                    }
                }
                .frame(width: 16, height: 16)
                .frame(width: 28, height: 24)
            }
            .buttonStyle(.plain)
            .disabled(selected == nil)
            .help("Open in \(selected?.name ?? "Application")")
            .accessibilityLabel("Open in \(selected?.name ?? "Application")")
            Divider().frame(height: 24)
            Menu {
                ForEach(applications) { application in
                    Button {
                        selectedID = application.id
                        selectedPath = application.url.path
                        open(application)
                    } label: {
                        Label {
                            Text(application.name)
                        } icon: {
                            Image(nsImage: application.icon)
                        }
                        .labelStyle(.titleAndIcon)
                    }
                }
                Divider()
                Button("Other Application…") { chooseApplication() }
                Divider()
                Button("Copy Path", systemImage: "doc.on.doc") {
                    guard let context = model.directoryContext else { return }
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(context.directory.path, forType: .string)
                }
            } label: {
                Image(systemName: "chevron.down")
                    .font(.system(size: 10, weight: .medium))
                    .frame(width: 22, height: 24)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .help("Open With")
            .accessibilityLabel("Open With")
            .onHover { if $0 { reloadApplications() } }
        }
        .background(RoundedRectangle(cornerRadius: 5).strokeBorder(.secondary.opacity(0.4)))
        .disabled(model.directoryContext == nil)
        .onAppear(perform: reloadApplications)
        .onChange(of: model.directoryContext?.directory) { _ in reloadApplications() }
        .onChange(of: selectedPath) { _ in reloadApplications() }
        .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.didLaunchApplicationNotification)) { _ in
            reloadApplications()
        }
        .alert("Couldn’t Open Folder", isPresented: Binding(
            get: { error != nil }, set: { if !$0 { error = nil } }
        )) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    private func open(_ application: TerminalExternalApplication) {
        guard let context = model.directoryContext else { return }
        NSWorkspace.shared.open(
            [context.directory], withApplicationAt: application.url, configuration: .init()
        ) { _, launchError in
            if let launchError {
                DispatchQueue.main.async { error = launchError.localizedDescription }
            }
        }
    }

    private func reloadApplications() {
        guard let directory = model.directoryContext?.directory else { applications = []; return }
        applications = TerminalExternalApplication.available(for: directory, selectedPath: selectedPath)
    }

    private func chooseApplication() {
        let panel = NSOpenPanel()
        panel.message = "Choose an application to open this folder."
        panel.allowedContentTypes = [.applicationBundle]
        panel.allowsMultipleSelection = false
        panel.directoryURL = FileManager.default.urls(for: .applicationDirectory, in: .localDomainMask).first
        guard panel.runModal() == .OK, let url = panel.url,
              let application = TerminalExternalApplication(url: url) else { return }
        selectedID = application.id
        selectedPath = application.url.path
        reloadApplications()
        open(application)
    }
}

struct TerminalExternalApplication: Identifiable {
    let id: String
    let name: String
    let url: URL

    var icon: NSImage {
        let image = NSWorkspace.shared.icon(forFile: url.path)
        image.size = NSSize(width: 16, height: 16)
        return image
    }

    init?(url: URL) {
        guard url.isFileURL, url.pathExtension.lowercased() == "app", let bundle = Bundle(url: url),
              FileManager.default.fileExists(atPath: url.path) else { return nil }
        self.url = url.standardizedFileURL
        id = bundle.bundleIdentifier ?? self.url.path
        name = (bundle.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String)
            ?? (bundle.object(forInfoDictionaryKey: "CFBundleName") as? String)
            ?? url.deletingPathExtension().lastPathComponent
    }

    static func available(for directory: URL, selectedPath: String) -> [Self] {
        let workspace = NSWorkspace.shared
        let defaultURL = workspace.urlForApplication(toOpen: directory)
        var urls = workspace.urlsForApplications(toOpen: directory)
        if let defaultURL { urls.insert(defaultURL, at: 0) }
        if !selectedPath.isEmpty { urls.insert(URL(fileURLWithPath: selectedPath), at: 0) }
        var seen = Set<String>()
        return urls.compactMap(Self.init(url:)).filter { seen.insert($0.id).inserted }.sorted {
            if $0.url == defaultURL { return true }
            if $1.url == defaultURL { return false }
            return $0.name.localizedStandardCompare($1.name) == .orderedAscending
        }
    }
}
