enum HorizontalNewTabButtonPlacement: String, CaseIterable, Identifiable {
    case titlebar
    case tabbar

    static let defaultsKey = "HorizontalNewTabButtonPlacement"
    static let defaultValue = Self.tabbar

    var id: Self { self }

    var label: String {
        switch self {
        case .titlebar: return "Title Bar"
        case .tabbar: return "Tab Bar"
        }
    }

    func showsInTitlebar(sidebarVisible: Bool, titlebarAvailable: Bool) -> Bool {
        titlebarAvailable && (sidebarVisible || self == .titlebar)
    }
}
