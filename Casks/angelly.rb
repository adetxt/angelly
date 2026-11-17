cask "angelly" do
  version "0.18.0"
  sha256 "e1ae026df61634551bdcde8c358ea195a596bd3471b434665a1396e1af282551"

  url "https://github.com/adetxt/angelly/releases/download/v#{version}/Angelly-#{version}.dmg"
  name "Angelly"
  desc "Terminal emulator"
  homepage "https://github.com/adetxt/angelly"

  auto_updates true
  depends_on macos: :sonoma

  app "Angelly.app"

  zap trash: [
    "~/Library/Application Support/angelly",
    "~/Library/Caches/com.adetxt.angelly",
    "~/Library/Preferences/com.adetxt.angelly.plist",
    "~/Library/Saved Application State/com.adetxt.angelly.savedState",
  ]
end
