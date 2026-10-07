# winget manifests

These are the manifests for `winget install jvz-devx.ytfast`, for the Inno Setup installer that the release workflow builds (`ytfast-gpui-<version>-windows-x86_64-setup.exe`). They use manifest schema 1.12.0, the version winget-pkgs asks for.

- `jvz-devx.ytfast.yaml`: the version manifest.
- `jvz-devx.ytfast.installer.yaml`: the setup program. `InstallerType: inno` gives the silent switches (winget adds `/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART`), and `Custom` adds `/CLOSEAPPLICATIONS /NORESTARTAPPLICATIONS` so an upgrade closes a running copy. `Scope: user` because the setup installs per user (`PrivilegesRequired=lowest`). `ProductCode` is the .iss `AppId` with Inno's `_is1` suffix, which is how winget finds the installed copy.
- `jvz-devx.ytfast.locale.en-US.yaml`: name, description, MIT licence, links.

The files hold the v0.1.0-alpha.1 values. `komac analyze` read the installer type, scope, product code and install location from the setup program. The files validate against the 1.12.0 JSON schemas, and `komac submit --dry-run` parses them. `winget validate` needs Windows and hasn't been run.

## The identifier

`jvz-devx.ytfast`: the publisher as the installer reports it (`AppPublisher=jvz-devx`), then the product's short name. "Music" alone is too generic to be an identifier.

Every release so far is a pre-release, and winget-pkgs has no written rule against them. winget sorts `0.1.0-alpha.1` below `0.1.0`, so a later stable release still counts as the upgrade. Projects that ship both stable and pre-releases usually give the pre-releases their own identifier, such as `….Preview`, because otherwise winget moves stable users onto betas. With no stable release yet, that problem can't arise. Once the first stable release is out, submit later pre-releases as `jvz-devx.ytfast.Preview` instead.

## Submitting a version

Submissions are made by hand from the dev machine, as the maintainer's own GitHub account, through its fork `jvz-devx/winget-pkgs`. Nothing in GitHub Actions submits to winget, so no token is stored in GitHub and no bot account is needed.

```sh
scripts/winget-submit.sh 0.1.0-alpha.2            # dry run: prints the manifests
scripts/winget-submit.sh 0.1.0-alpha.2 --submit   # opens the pull request
```

The script runs komac (`komac` from `PATH`, otherwise `nix shell nixpkgs#komac`). It takes the token from `gh auth token` each time it runs and passes it to komac only through komac's environment, never on a command line, in a file or in the output. The account `gh` is signed in to needs the `public_repo` scope; the `repo` scope that `gh auth login` grants by default includes it.

- **First submission** (the package isn't in winget-pkgs yet): the script submits the reviewed files in this folder. They must already be for the version you pass.
- **Later versions**: `komac update` builds the new version from the release's setup program and carries everything else over from the newest version in winget-pkgs. Run the dry run first and read the output, then run again with `--submit`.

A winget-pkgs bot validates each pull request with antivirus scans and a test install, and a moderator reviews new packages, so the first one can take a few days. The package is available once the pull request is merged.

## Before the first submission

The maintainer decides when to submit the first version. Before that:

- `gh auth status` shows the maintainer's account with the `repo` or `public_repo` scope.
- `jvz-devx/winget-pkgs` exists and is up to date (`komac sync` updates it).
- `scripts/winget-submit.sh 0.1.0-alpha.1` (the dry run) shows the manifests above.
