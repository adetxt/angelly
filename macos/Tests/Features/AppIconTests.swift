import AppKit
import Testing
@testable import Ghostty

struct AppIconTests {
    @Test func officialUsesBundledArtwork() throws {
        let config = try TemporaryConfig("macos-icon = official")
        #expect(AppIcon(config: config) == .official)
        #expect(AppIcon.official.image(in: .main) != nil)
        #expect(Ghostty.MacOSIcon.official.assetName == "AppIconImage")
    }

    @Test func bundledIconsLoadFromConfigAndAssets() throws {
        #expect(Ghostty.MacOSIcon.bundledIcons.count == 10)
        for style in Ghostty.MacOSIcon.bundledIcons {
            let config = try TemporaryConfig("macos-icon = \(style.rawValue)")
            #expect(config.macosIcon == style)
            let icon = try #require(AppIcon(config: config))
            #expect(icon.image(in: .main) != nil)
        }
        #expect(!Ghostty.MacOSIcon.bundledIcons.contains(.custom))
        #expect(!Ghostty.MacOSIcon.bundledIcons.contains(.customStyle))
    }
}
