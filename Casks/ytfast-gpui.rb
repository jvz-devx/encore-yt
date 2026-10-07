cask "ytfast-gpui" do
  arch arm: "arm64", intel: "x86_64"

  version "0.1.0-alpha.1"
  sha256 arm:   "d607d7b5cf92ae805f12b74094a4d900ee5986a1f9a55c8f01821756c8b7940f",
         intel: "c02744fb0a422d7a1eea7d411e9c188303a5ae49a603018ced0114555f318c2b"

  depends_on macos: ">= :big_sur"

  url "https://github.com/jvz-devx/ytfast-gpui/releases/download/v#{version}/ytfast-gpui-#{version}-macos-#{arch}.dmg"
  name "Music (ytfast)"
  desc "Native YouTube Music client"
  homepage "https://github.com/jvz-devx/ytfast-gpui"

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

  app "ytfast.app"

  # The app is ad-hoc signed, not notarized: with the quarantine flag that
  # Homebrew sets on the download, Gatekeeper refuses to open it.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/ytfast.app"]
  end

  zap trash: [
    "~/Library/Application Support/ytfast",
    "~/Library/Caches/ytfast",
    "~/Library/Preferences/io.github.jvz-devx.ytfast-gpui.plist",
    "~/Library/Saved Application State/io.github.jvz-devx.ytfast-gpui.savedState",
  ]
end
