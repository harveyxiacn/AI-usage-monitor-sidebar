# Homebrew cask for the unsigned, un-notarised macOS builds.
#
# TODO before publishing: replace both REPLACE_WITH_SHA256 values. Get them
# with, for the release you are packaging:
#
#   shasum -a 256 AI.Usage.Sidebar_0.1.1_aarch64.dmg
#   shasum -a 256 AI.Usage.Sidebar_0.1.1_x64.dmg
#
# `brew bump-cask-pr` rewrites version and sha256 for later releases.
cask "ai-usage-sidebar" do
  version "0.1.1"

  on_arm do
    sha256 "REPLACE_WITH_SHA256_OF_THE_AARCH64_DMG"
    url "https://github.com/harveyxiacn/AI-usage-monitor-sidebar/releases/download/v#{version}/AI.Usage.Sidebar_#{version}_aarch64.dmg",
        verified: "github.com/harveyxiacn/AI-usage-monitor-sidebar/"
  end
  on_intel do
    sha256 "REPLACE_WITH_SHA256_OF_THE_X64_DMG"
    url "https://github.com/harveyxiacn/AI-usage-monitor-sidebar/releases/download/v#{version}/AI.Usage.Sidebar_#{version}_x64.dmg",
        verified: "github.com/harveyxiacn/AI-usage-monitor-sidebar/"
  end

  name "AI Usage Sidebar"
  desc "Edge sidebar showing Claude Code and Codex rate-limit quotas"
  homepage "https://github.com/harveyxiacn/AI-usage-monitor-sidebar"

  livecheck do
    url :url
    strategy :github_latest
  end

  # The builds are not signed with a Developer ID, so Gatekeeper quarantines
  # them. Homebrew removes the quarantine flag for casks it installs itself.
  auto_updates false
  depends_on macos: ">= :big_sur"

  app "AI Usage Sidebar.app"

  uninstall quit: "io.github.harveyxiacn.ai-usage-sidebar"

  # Settings and the local usage database; `brew uninstall --zap` removes them.
  zap trash: [
    "~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar",
    "~/Library/Logs/io.github.harveyxiacn.ai-usage-sidebar",
    "~/Library/Preferences/io.github.harveyxiacn.ai-usage-sidebar.plist",
    "~/Library/Saved Application State/io.github.harveyxiacn.ai-usage-sidebar.savedState",
  ]
end
