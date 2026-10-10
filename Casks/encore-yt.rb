cask "encore-yt" do
  arch arm: "arm64", intel: "x86_64"

  version "1.0.0-beta.3"
  sha256 arm:   "15b815532621d78c156b9224301b80794035f779dbf90291d8e014dec791e99f",
         intel: "89ebc34a754bc6c64ba9d444af1de5e51d5940a37a8e7a8da69619222dcf11c4"

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
