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

/// Both tab layouts share the creation menu and open terminals after the sheet closes.
struct TerminalNewTabMenu: View {
    @ObservedObject var tabs: TerminalTabModel
    @State private var isPresented = false
    @State private var sourceDirectory: URL?
    @State private var directoryToOpen: URL?

    var body: some View {
        Menu {
            Button("New Tab") { tabs.newTab() }
            Button("Create Worktree…") {
                sourceDirectory = tabs.workingDirectory
                isPresented = true
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
        .sheet(isPresented: $isPresented, onDismiss: openPendingDirectory) {
            WorktreeManagerView(directory: sourceDirectory) { directory in
                directoryToOpen = directory
                isPresented = false
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
    let open: (URL) -> Void
    @Environment(\.dismiss) private var dismiss
    @AppStorage(WorktreePreferences.rootKey, store: UserDefaults.ghostty)
    private var rootPath = WorktreePreferences.defaultRoot
    @State private var sourceDirectory: URL?
    @State private var snapshot: GitWorktreeSnapshot?
    @State private var isBusy = false
    @State private var error: String?
    @State private var newBranch = true
    @State private var branch = ""
    @State private var existingBranch = ""
    @State private var folderName = ""
    @State private var removing: GitWorktree?

    init(directory: URL?, open: @escaping (URL) -> Void) {
        self.open = open
        _sourceDirectory = State(initialValue: directory)
    }

    private var availableBranches: [String] {
        guard let snapshot else { return [] }
        let checkedOut = Set(snapshot.worktrees.compactMap(\.branch))
        return snapshot.branches.filter { !checkedOut.contains($0) }
    }

    private var selectedBranch: String {
        newBranch ? branch.trimmingCharacters(in: .whitespacesAndNewlines) : existingBranch
    }

    private var selectedFolder: String {
        folderName.isEmpty ? selectedBranch.replacingOccurrences(of: "/", with: "-") : folderName
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Worktrees").font(.title2)
                Spacer()
                Button("Choose Repository…", action: chooseRepository)
                Button("Refresh") { Task { await refresh() } }
                    .disabled(sourceDirectory == nil)
            }

            if let snapshot {
                Text(snapshot.directory.path)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                worktreeList(snapshot)
                creationForm(snapshot)
            } else {
                Text("Open a terminal in a Git repository, or choose a repository to manage its worktrees.")
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
        ScrollView {
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
        .frame(height: 200)
    }

    private func creationForm(_ snapshot: GitWorktreeSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Create Worktree").font(.headline)
            Picker("Branch", selection: $newBranch) {
                Text("New branch").tag(true)
                Text("Existing branch").tag(false)
            }
            .pickerStyle(.segmented)

            if newBranch {
                TextField("Branch name", text: $branch)
                Text("The new branch starts at HEAD of the selected checkout.")
                    .font(.caption).foregroundStyle(.secondary)
            } else if availableBranches.isEmpty {
                Text("All local branches are already checked out. Create a new branch instead.")
                    .font(.callout).foregroundStyle(.secondary)
            } else {
                Picker("Branch name", selection: $existingBranch) {
                    ForEach(availableBranches, id: \.self) { Text($0).tag($0) }
                }
            }

            TextField("Folder name", text: $folderName, prompt: Text("Defaults to branch name"))
            Text(URL(fileURLWithPath: rootPath).appendingPathComponent(snapshot.directory.lastPathComponent)
                .appendingPathComponent(selectedFolder).path)
                .font(.caption)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)

            HStack {
                Text("Parent folder can be changed in Settings → Worktrees.")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Create and Open") { Task { await create() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(selectedBranch.isEmpty || (!newBranch && !availableBranches.contains(existingBranch)))
            }
        }
    }

    private func chooseRepository() {
        guard let directory = WorktreePreferences.chooseFolder(message: "Choose a Git repository.") else { return }
        sourceDirectory = directory
        snapshot = nil
        Task { await refresh() }
    }

    private func refresh() async {
        guard !isBusy, let directory = sourceDirectory ?? snapshot?.directory else { return }
        isBusy = true
        defer { isBusy = false }
        do {
            snapshot = try await GitWorktreeService.shared.load(from: directory)
            error = nil
            if !availableBranches.contains(existingBranch) { existingBranch = availableBranches.first ?? "" }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func create() async {
        guard !isBusy, let snapshot else { return }
        isBusy = true
        defer { isBusy = false }
        error = nil
        do {
            let directory = try await GitWorktreeService.shared.create(
                in: sourceDirectory ?? snapshot.directory, root: URL(fileURLWithPath: rootPath),
                folderName: selectedFolder, branch: selectedBranch, newBranch: newBranch)
            openDirectory(directory)
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func remove(_ worktree: GitWorktree) async {
        guard !isBusy, let snapshot else { return }
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
            if let source = sourceDirectory?.resolvingSymlinksInPath().path,
               source == worktree.directory.path || source.hasPrefix(prefix) {
                sourceDirectory = snapshot.directory
            }
            self.snapshot = try await GitWorktreeService.shared.load(from: snapshot.directory)
            if !availableBranches.contains(existingBranch) { existingBranch = availableBranches.first ?? "" }
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
