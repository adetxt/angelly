import Testing
@testable import Ghostty

@Suite
struct WorktreeCreateViewTests {
    @Test func defaultFolderNameFlattensBranchSeparators() {
        #expect(WorktreeCreateView.defaultFolderName(forBranch: "feature/login/ui") == "feature-login-ui")
        #expect(WorktreeCreateView.defaultFolderName(forBranch: "main") == "main")
    }
}
