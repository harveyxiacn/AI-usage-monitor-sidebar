# Homebrew cask for the unsigned, un-notarised macOS builds.
#
# The sha256 values are those of the published release assets for `version`
# (GitHub reports them as the asset `digest`; `shasum -a 256 <file>.dmg` gives
# the same). They must be refreshed together with `version`.
#
# `brew bump-cask-pr` rewrites version and sha256 for later releases.
cask "ai-usage-sidebar" do
  version "0.2.0"

  on_arm do
    sha256 "fb14d1f538a01396b62e5f88d9635e494a09ebf2263e8e180c0b0007d3d45afd"
    url "https://github.com/harveyxiacn/AI-usage-monitor-sidebar/releases/download/v#{version}/AI.Usage.Sidebar_#{version}_aarch64.dmg",
        verified: "github.com/harveyxiacn/AI-usage-monitor-sidebar/"
  end
  on_intel do
    sha256 "771431d8d9c4a69cb7c881a68a10349feec5218c0903f9a057a8baba9bd566a5"
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
