import Testing
@testable import Ghostty

@Suite
struct TerminalTabPreferencesTests {
    @Test func horizontalTabsDefaultToTabBar() {
        #expect(HorizontalNewTabButtonPlacement.defaultValue == .tabbar)
        #expect(!HorizontalNewTabButtonPlacement.defaultValue.showsInTitlebar(
            sidebarVisible: false, titlebarAvailable: true))
        #expect(HorizontalNewTabButtonPlacement.titlebar.showsInTitlebar(
            sidebarVisible: false, titlebarAvailable: true))
    }

    @Test func verticalTabsKeepNewButtonInTitlebar() {
        for placement in HorizontalNewTabButtonPlacement.allCases {
            #expect(placement.showsInTitlebar(sidebarVisible: true, titlebarAvailable: true))
        }
    }

    @Test func hiddenTitlebarCannotShowNewButton() {
        for placement in HorizontalNewTabButtonPlacement.allCases {
            for sidebarVisible in [false, true] {
                #expect(!placement.showsInTitlebar(
                    sidebarVisible: sidebarVisible, titlebarAvailable: false))
            }
        }
    }
}
