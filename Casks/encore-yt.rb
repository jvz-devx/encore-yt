cask "encore-yt" do
  arch arm: "arm64", intel: "x86_64"

  version "0.2.0-alpha.2"
  sha256 arm:   "f90a0df3aacfeeb05a65ffcb6d45a9de622dc5642a2e6743c1ba4f62d4833795",
         intel: "6c703b99d1a826863490608b81cda143df71da6e3606bc7e420d52acd71aa99c"

  depends_on macos: ">= :big_sur"

  url "https://github.com/jvz-devx/encore-yt/releases/download/v#{version}/encore-yt-#{version}-macos-#{arch}.dmg"
  name "Encore"
  desc "Native YouTube Music client"
  homepage "https://github.com/jvz-devx/encore-yt"

  # Every release so far is a pre-release, which the default
  # :github_releases strategy (and :github_latest) skip.
  livecheck do
    url :url
    strategy :github_releases do |json|
      json.map do |release|
        next if release["draft"]

        release["tag_name"]&.delete_prefix("v")
      end
    end
  end

  # The app updates itself from GitHub Releases (Settings, Updates).
  auto_updates true

  app "Encore.app"

  # The app is ad-hoc signed, not notarized: with the quarantine flag that
  # Homebrew sets on the download, Gatekeeper refuses to open it.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/Encore.app"]
  end

  zap trash: [
    "~/Library/Application Support/encore-yt",
    "~/Library/Caches/encore-yt",
    "~/Library/Preferences/io.github.jvz-devx.encore-yt.plist",
    "~/Library/Saved Application State/io.github.jvz-devx.encore-yt.savedState",
  ]
end
