cask "ytfast-gpui" do
  arch arm: "arm64", intel: "x86_64"

  version "0.1.0-alpha.2"
  sha256 arm:   "7bdd217fa310773494fe481aa0dce12afb99f4ae38a44787e3cbb64de56ff92b",
         intel: "d1c2c61369bdaadf11cdcff875bb618c58ece6c3d45e303c9eaefe31d9c20e01"

  # The bundled mpv needs macOS 14 on Apple silicon and 15 on Intel.
  on_arm do
    depends_on macos: :sonoma
  end
  on_intel do
    depends_on macos: :sequoia
  end

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
