// This file contains the configuration types for Ghostty so that alternate targets
// can get typed information without depending on all the dependencies of GhosttyKit.

extension Ghostty {
    /// A configuration path value that may be optional or required.
    struct ConfigPath: Sendable {
        let path: String
        let optional: Bool
    }

    /// macos-icon
    enum MacOSIcon: String, Sendable, CaseIterable {
        case official
        case blueprint
        case chalkboard
        case egyptian
        case glass
        case holographic
        case microchip
        case paper
        case retro
        case xray
        case custom
        case customStyle = "custom-style"

        static var bundledIcons: [Self] { allCases.filter { $0.assetName != nil } }

        /// Bundled asset name for built-in icons
        var assetName: String? {
            switch self {
            case .official: return "AppIconImage"
            case .blueprint: return "BlueprintImage"
            case .chalkboard: return "ChalkboardImage"
            case .egyptian: return "EgyptianImage"
            case .microchip: return "MicrochipImage"
            case .glass: return "GlassImage"
            case .holographic: return "HolographicImage"
            case .paper: return "PaperImage"
            case .retro: return "RetroImage"
            case .xray: return "XrayImage"
            case .custom, .customStyle: return nil
            }
        }
    }

    /// macos-icon-frame
    enum MacOSIconFrame: String, Codable {
        case aluminum
        case beige
        case plastic
        case chrome
    }
}
