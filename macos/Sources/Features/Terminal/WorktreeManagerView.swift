import AppKit
import SwiftUI

enum WorktreePreferences {
    static let rootKey = "WorktreeRoot"
    static let defaultRoot = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("worktrees", isDirectory: true).path

    static func chooseFolder(message: String, startingAt path: String? = nil) -> URL? {
        let panel = NSOpenPanel()
        panel.message = message
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        if let path { panel.directoryURL = URL(fileURLWithPath: path) }
        return panel.runModal() == .OK ? panel.url : nil
    }
}

struct WorktreeFolderSetting: View {
    @AppStorage(WorktreePreferences.rootKey, store: UserDefaults.ghostty)
    private var rootPath = WorktreePreferences.defaultRoot

    var body: some View {
        LabeledContent("Parent folder") {
            Text(rootPath)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(rootPath)
            Button("Choose…") {
                if let folder = WorktreePreferences.chooseFolder(
                    message: "Choose where Angelly creates worktrees.", startingAt: rootPath) {
                    rootPath = folder.path
                }
            }
            Button("Reset") { rootPath = WorktreePreferences.defaultRoot }
                .disabled(rootPath == WorktreePreferences.defaultRoot)
        }
        Text("New worktrees are placed inside a folder named after the repository. Changing this setting does not move existing worktrees.")
            .font(.caption)
            .foregroundStyle(.secondary)
    }
}

/// Both tab layouts share the creation menu and open terminals after the popover or sheet closes.
struct TerminalNewTabMenu: View {
    @ObservedObject var tabs: TerminalTabModel
    @State private var isCreating = false
    @State private var isManaging = false
    @State private var sourceDirectory: URL?
    @State private var directoryToOpen: URL?

    var body: some View {
        Menu {
            Button("New Tab") { tabs.newTab() }
            Divider()
            Button("New Worktree…") {
                sourceDirectory = tabs.workingDirectory
                isCreating = true
            }
            Button("Manage Worktrees…") {
                sourceDirectory = tabs.workingDirectory
                isManaging = true
            }
        } label: {
            Image(systemName: "plus")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 24)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .help("New Tab or Worktree")
        .accessibilityLabel("New Tab or Worktree")
        .popover(isPresented: $isCreating, arrowEdge: .bottom) {
            WorktreeCreateView(directory: sourceDirectory) { directory in
                directoryToOpen = directory
                isCreating = false
            }
        }
        .onChange(of: isCreating) { presented in
            if !presented { openPendingDirectory() }
        }
        .sheet(isPresented: $isManaging, onDismiss: openPendingDirectory) {
            WorktreeManagerView(directory: sourceDirectory) { directory in
                directoryToOpen = directory
                isManaging = false
            }
        }
    }

    private func openPendingDirectory() {
        guard let directory = directoryToOpen else { return }
        directoryToOpen = nil
        tabs.openWorktree(at: directory)
    }
}

private struct WorktreeManagerView: View {
    let directory: URL?
    let open: (URL) -> Void
    @Environment(\.dismiss) private var dismiss
    @AppStorage(WorktreePreferences.rootKey, store: UserDefaults.ghostty)
    private var rootPath = WorktreePreferences.defaultRoot
    @State private var repositories: [GitWorktreeSnapshot] = []
    @State private var selectedRepository: URL?
    @State private var isBusy = false
    @State private var error: String?
    @State private var removing: GitWorktree?

    private var visibleRepositories: [GitWorktreeSnapshot] {
        repositories.filter { selectedRepository == nil || $0.directory == selectedRepository }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Manage Worktrees").font(.title2)
                Spacer()
                Button("Refresh") { Task { await refresh() } }
            }

            HStack {
                Picker("Repository", selection: $selectedRepository) {
                    Text("All repositories").tag(Optional<URL>.none)
                    ForEach(repositories, id: \.directory) { snapshot in
                        Text(snapshot.directory.path).tag(Optional(snapshot.directory))
                    }
                }
                Button("Choose Repository…", action: chooseRepository)
            }

            if !repositories.isEmpty {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        ForEach(visibleRepositories, id: \.directory) { snapshot in
                            VStack(alignment: .leading, spacing: 4) {
                                Text(snapshot.directory.lastPathComponent).font(.headline)
                                Text(snapshot.directory.path)
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                                    .textSelection(.enabled)
                                worktreeList(snapshot)
                            }
                        }
                    }
                }
                .frame(height: 260)
            } else {
                Text("No repositories found in open terminals or the worktree folder. Choose a repository to manage its worktrees.")
                    .frame(maxWidth: .infinity, minHeight: 120, alignment: .center)
            }

            if let error {
                ScrollView {
                    Text(error)
                        .foregroundStyle(.red)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 90)
                .accessibilityLabel("Git error: \(error)")
            }

            HStack {
                if isBusy { ProgressView("Running Git…").controlSize(.small) }
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
        }
        .padding(20)
        .frame(width: 680)
        .disabled(isBusy)
        .interactiveDismissDisabled(isBusy)
        .task { await refresh() }
        .alert("Remove Worktree?", isPresented: Binding(
            get: { removing != nil },
            set: { if !$0 { removing = nil } }
        ), presenting: removing) { worktree in
            Button("Remove Worktree", role: .destructive) {
                Task { await remove(worktree) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { worktree in
            Text("Remove \(worktree.directory.path)? Its branch will be kept. Removal is refused if it contains uncommitted, untracked, or ignored files.")
        }
    }

    private func worktreeList(_ snapshot: GitWorktreeSnapshot) -> some View {
        VStack(spacing: 0) {
            ForEach(snapshot.worktrees) { worktree in
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(worktree.branch ?? (worktree.isBare ? "Bare repository" : "Detached HEAD"))
                            .fontWeight(.medium)
                        Text(worktree.directory.path)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                        if worktree.isMain { Text("Main checkout").font(.caption) }
                        if worktree.isLocked { Text("Locked").font(.caption) }
                        if worktree.isPrunable { Text("Missing directory").font(.caption) }
                    }
                    Spacer(minLength: 8)
                    Button("Open Tab") { openDirectory(worktree.directory) }
                        .disabled(worktree.isBare || worktree.isPrunable)
                        .accessibilityLabel("Open \(worktree.branch ?? worktree.directory.lastPathComponent) in a new tab")
                    Button("Remove…", role: .destructive) { removing = worktree }
                        .disabled(!worktree.canRemove)
                        .accessibilityLabel("Remove worktree \(worktree.directory.lastPathComponent)")
                }
                .padding(.vertical, 10)
                Divider()
            }
        }
    }

    private func chooseRepository() {
        guard let directory = WorktreePreferences.chooseFolder(message: "Choose a Git repository.") else { return }
        Task {
            guard !isBusy else { return }
            isBusy = true
            defer { isBusy = false }
            do {
                let snapshot = try await GitWorktreeService.shared.load(from: directory)
                repositories.removeAll { $0.directory == snapshot.directory }
                repositories.append(snapshot)
                repositories.sort { $0.directory.path.localizedStandardCompare($1.directory.path) == .orderedAscending }
                selectedRepository = snapshot.directory
                error = nil
            } catch {
                self.error = error.localizedDescription
            }
        }
    }

    private func refresh() async {
        guard !isBusy else { return }
        isBusy = true
        defer { isBusy = false }
        var directories = repositories.map(\.directory)
        if let directory { directories.append(directory) }
        for case let controller as BaseTerminalController in NSApp.windows.compactMap(\.windowController) {
            for surface in controller.surfaceTree {
                if let pwd = surface.pwd, pwd.hasPrefix("/") {
                    directories.append(URL(fileURLWithPath: pwd))
                }
            }
        }
        repositories = await GitWorktreeService.shared.loadRepositories(
            from: directories, worktreeRoot: URL(fileURLWithPath: rootPath))
        if let selectedRepository, !repositories.contains(where: { $0.directory == selectedRepository }) {
            self.selectedRepository = nil
        }
        error = nil
    }

    private func remove(_ worktree: GitWorktree) async {
        guard !isBusy, let index = repositories.firstIndex(where: { snapshot in
            snapshot.worktrees.contains { $0.directory == worktree.directory }
        }) else { return }
        let snapshot = repositories[index]
        let prefix = worktree.directory.path + "/"
        for case let controller as BaseTerminalController in NSApp.windows.compactMap(\.windowController) {
            for surface in controller.surfaceTree {
                guard let pwd = surface.pwd else { continue }
                let path = URL(fileURLWithPath: pwd).resolvingSymlinksInPath().path
                if path == worktree.directory.path || path.hasPrefix(prefix) {
                    error = "A terminal is still using this worktree. Close it or change its directory before removing the worktree."
                    return
                }
            }
        }
        isBusy = true
        error = nil
        do {
            try await GitWorktreeService.shared.remove(in: snapshot.directory, directory: worktree.directory)
            repositories[index] = try await GitWorktreeService.shared.load(from: snapshot.directory)
        } catch {
            self.error = error.localizedDescription
        }
        isBusy = false
    }

    private func openDirectory(_ directory: URL) {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: directory.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            error = "This worktree directory is unavailable. Refresh the list and try again."
            return
        }
        open(directory)
    }
}
