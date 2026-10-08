cask "encore-yt" do
  arch arm: "arm64", intel: "x86_64"

  version "1.0.0-beta.1"
  sha256 arm:   "c454f85d36beb9133bdbf9f4dadfa343f0a2381436895d40ba94860dfb68cd35",
         intel: "3823690978c83400e1a72019e8bc38ecb436442d1dcf39e0588fdf4abac15d24"

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
