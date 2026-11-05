import SwiftUI

/// Compact popover for the common case: type a branch name, press Return.
struct WorktreeCreateView: View {
    let directory: URL?
    let open: (URL) -> Void

    @AppStorage(WorktreePreferences.rootKey, store: UserDefaults.ghostty)
    private var rootPath = WorktreePreferences.defaultRoot
    @State private var snapshot: GitWorktreeSnapshot?
    @State private var isBusy = false
    @State private var error: String?
    @State private var usesExistingBranch = false
    @State private var branch = ""
    @State private var existingBranch = ""
    @State private var folderName = ""
    @State private var showsOptions = false
    @FocusState private var branchFocused: Bool

    /// Git branches may contain `/`, but a worktree folder is always one flat path component.
    static func defaultFolderName(forBranch branch: String) -> String {
        branch.replacingOccurrences(of: "/", with: "-")
    }

    private var availableBranches: [String] {
        guard let snapshot else { return [] }
        let checkedOut = Set(snapshot.worktrees.compactMap(\.branch))
        return snapshot.branches.filter { !checkedOut.contains($0) }
    }

    private var selectedBranch: String {
        usesExistingBranch ? existingBranch : branch.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var selectedFolder: String {
        folderName.isEmpty ? Self.defaultFolderName(forBranch: selectedBranch) : folderName
    }

    private var canCreate: Bool { snapshot != nil && !selectedBranch.isEmpty && !isBusy }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("New Worktree").font(.headline)

            if let snapshot {
                branchField
                Text(destination(for: snapshot))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(destination(for: snapshot))
                // A plain button, so the whole row toggles; DisclosureGroup only reacts to its chevron.
                Button {
                    showsOptions.toggle()
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "chevron.right")
                            .font(.caption.weight(.semibold))
                            .rotationEffect(.degrees(showsOptions ? 90 : 0))
                            .frame(width: 10)
                        Text("Options")
                        Spacer()
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityValue(showsOptions ? "Expanded" : "Collapsed")
                if showsOptions {
                    labeled("Folder name") {
                        TextField("Folder name", text: $folderName,
                                  prompt: Text(Self.defaultFolderName(forBranch: selectedBranch)))
                            .labelsHidden()
                    }
                }
            } else if !isBusy {
                Text("Open a terminal in a Git repository to create a worktree.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }

            if let error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
                    .accessibilityLabel("Git error: \(error)")
            }

            HStack {
                if isBusy { ProgressView().controlSize(.small) }
                Spacer()
                Button("Create and Open") { Task { await create() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canCreate)
            }
        }
        .padding(16)
        .frame(width: 340)
        .task { await load() }
    }

    @ViewBuilder
    private var branchField: some View {
        if usesExistingBranch {
            labeled("Branch") {
                if availableBranches.isEmpty {
                    Text("Every local branch is already checked out.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                } else {
                    Picker("Branch", selection: $existingBranch) {
                        ForEach(availableBranches, id: \.self) { Text($0).tag($0) }
                    }
                    .labelsHidden()
                }
            }
        } else {
            labeled("Branch name") {
                TextField("Branch name", text: $branch, prompt: Text("feature/my-change"))
                    .labelsHidden()
                    .focused($branchFocused)
                    .onSubmit { if canCreate { Task { await create() } } }
                    .onAppear { branchFocused = true }
            }
        }
        Button(usesExistingBranch ? "Create a new branch instead" : "Use an existing branch…") {
            usesExistingBranch.toggle()
            error = nil
        }
        .buttonStyle(.link)
        .font(.caption)
    }

    private func labeled<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.subheadline).foregroundStyle(.secondary)
            content()
        }
    }

    private func destination(for snapshot: GitWorktreeSnapshot) -> String {
        URL(fileURLWithPath: rootPath)
            .appendingPathComponent(snapshot.directory.lastPathComponent)
            .appendingPathComponent(selectedFolder).path
    }

    private func load() async {
        guard let directory else { return }
        isBusy = true
        defer { isBusy = false }
        do {
            let snapshot = try await GitWorktreeService.shared.load(from: directory)
            self.snapshot = snapshot
            existingBranch = availableBranches.first ?? ""
        } catch {
            // Not a repository: the message above already tells the user what to do.
        }
    }

    private func create() async {
        guard let snapshot, canCreate else { return }
        isBusy = true
        defer { isBusy = false }
        error = nil
        do {
            let created = try await GitWorktreeService.shared.create(
                in: directory ?? snapshot.directory, root: URL(fileURLWithPath: rootPath),
                folderName: selectedFolder, branch: selectedBranch, newBranch: !usesExistingBranch)
            open(created)
        } catch {
            self.error = error.localizedDescription
            // Folder collisions (`a/b` vs `a-b`) are fixed in Options, so reveal it.
            showsOptions = true
        }
    }
}
