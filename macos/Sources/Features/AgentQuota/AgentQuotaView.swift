import AppKit
import SwiftUI

struct AgentQuotaFooter: View {
    @ObservedObject var store: AgentQuotaStore
    @ObservedObject private var sidebar = TabSidebarState.shared
    @AppStorage(AgentQuotaPreferences.visibilityKey, store: UserDefaults.ghostty) private var showsQuota = true
    @State private var observerID = UUID()
    @State private var isMounted = false

    var body: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 2) {
                if showsQuota {
                    ForEach(AgentQuotaProvider.allCases) { provider in
                        AgentQuotaButton(provider: provider, store: store)
                    }
                }
                Spacer(minLength: 0)
                Button {
                    (NSApp.delegate as? AppDelegate)?.openConfig(nil)
                } label: {
                    Image(systemName: "gearshape")
                        .frame(width: 30, height: 30)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .quotaFocusRingHidden()
                .help("Settings")
                .accessibilityLabel("Settings")
                .accessibilityIdentifier("Sidebar Settings")
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
        }
        .onAppear {
            isMounted = true
            updateObservation()
        }
        .onDisappear {
            isMounted = false
            store.stopObserving(observerID)
        }
        .onChange(of: showsQuota) { _ in updateObservation() }
        .onChange(of: sidebar.isVisible) { _ in updateObservation() }
    }

    private func updateObservation() {
        if isMounted && showsQuota && sidebar.isVisible {
            store.observe(observerID)
        } else {
            store.stopObserving(observerID)
        }
    }
}

private struct AgentQuotaButton: View {
    let provider: AgentQuotaProvider
    @ObservedObject var store: AgentQuotaStore
    @State private var showsPopover = false

    private var state: AgentQuotaState { store.state(for: provider) }

    private var detail: String {
        if state.isLoading { return "Loading quota" }
        if let window = state.snapshot?.ringWindow {
            return "\(window.kind.label): \(window.remainingPercent.formatted(.number.precision(.fractionLength(0))))% remaining"
        }
        return state.error?.message(for: provider) ?? "Quota not loaded"
    }

    var body: some View {
        Button {
            showsPopover.toggle()
        } label: {
            ZStack {
                Circle().stroke(Color.primary.opacity(0.15), lineWidth: 2.5)
                if let window = state.snapshot?.ringWindow {
                    Circle()
                        .trim(from: 0, to: window.remainingPercent / 100)
                        .stroke(Color.primary, style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                }
                Image(provider.logo)
                    .resizable()
                    .scaledToFit()
                    .frame(width: 12, height: 12)
                    .foregroundStyle(.primary)
                    .accessibilityHidden(true)
            }
            .frame(width: 22, height: 22)
            .frame(width: 30, height: 30)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .quotaFocusRingHidden()
        .help("\(provider.name)\n\(detail)")
        .accessibilityLabel("\(provider.name) quota")
        .accessibilityValue(detail)
        .accessibilityIdentifier("\(provider.rawValue) quota")
        .popover(isPresented: $showsPopover, arrowEdge: .trailing) {
            AgentQuotaPopover(provider: provider, store: store)
                .onExitCommand { showsPopover = false }
        }
        .onChange(of: showsPopover) { open in
            if open { store.refresh(provider) }
        }
    }
}

private struct AgentQuotaPopover: View {
    let provider: AgentQuotaProvider
    @ObservedObject var store: AgentQuotaStore
    @FocusState private var focused: Bool

    private var state: AgentQuotaState { store.state(for: provider) }

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            VStack(alignment: .leading, spacing: 14) {
                Text(provider.name).font(.headline)
                if state.isLoading {
                    HStack {
                        ProgressView().controlSize(.small)
                        Text("Loading quota…")
                    }
                } else if let snapshot = state.snapshot {
                    if let window = snapshot.fiveHour { quotaWindow(window) }
                    if let window = snapshot.weekly { quotaWindow(window) }
                    if snapshot.fiveHour == nil {
                        Text("The ring shows weekly quota because no 5-hour quota was reported.")
                            .font(.caption)
                    }
                } else {
                    Text((state.error ?? .unavailable).message(for: provider))
                        .fixedSize(horizontal: false, vertical: true)
                }

                if let updated = state.lastUpdated {
                    Text("Last updated \(updated.formatted(date: .omitted, time: .shortened))")
                        .font(.caption)
                }
                if let retry = state.retryAfter, retry > context.date {
                    Text("Refresh available \(retry, style: .relative)")
                        .font(.caption)
                }
                HStack {
                    if state.error == .permissionRequired {
                        Button("Allow Keychain Access…") {
                            store.refresh(provider, allowInteraction: true, force: true)
                        }
                        .focusable()
                    }
                    Spacer()
                    Button("Refresh") { store.refresh(provider, force: true) }
                        .keyboardShortcut(.defaultAction)
                        .focusable()
                        .disabled(state.isLoading || (state.retryAfter.map { $0 > context.date } ?? false))
                }
            }
            .padding(16)
            .frame(width: 300, alignment: .leading)
            .focusable()
            .quotaFocusRingHidden()
            .focused($focused)
            .onAppear { DispatchQueue.main.async { focused = true } }
        }
    }

    private func quotaWindow(_ window: AgentQuotaWindow) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack {
                Text(window.kind.label)
                Spacer()
                Text("\(window.remainingPercent.formatted(.number.precision(.fractionLength(0))))% remaining")
                    .monospacedDigit()
            }
            ProgressView(value: window.remainingPercent, total: 100)
                .tint(.primary)
                .accessibilityLabel("\(window.kind.label) quota remaining")
                .accessibilityValue("\(window.remainingPercent.formatted(.number.precision(.fractionLength(0)))) percent")
            if let reset = window.resetsAt {
                Text("Resets \(reset.formatted(date: .abbreviated, time: .shortened))")
                    .font(.caption)
            } else {
                Text("Reset time unavailable").font(.caption)
            }
        }
    }
}

private extension View {
    /// The footer has no focus ring, and on the popover container it would frame the whole card.
    @ViewBuilder
    func quotaFocusRingHidden() -> some View {
        if #available(macOS 14, *) { focusEffectDisabled() } else { self }
    }
}
