cask "ncrs" do
  # A single executable, not an .app bundle: ncrs is a terminal-style file
  # manager drawn with iced, and there is no window bundle to put in
  # /Applications. The cask therefore links the binary into the Homebrew
  # prefix, and `brew uninstall` removes exactly that one file.
  version "0.1.0"

  # Both hashes, because Homebrew picks by the machine it runs on: a cask with
  # only one sha256 fails on the other architecture, and the failure only shows
  # up on a machine the author does not own.
  sha256 arm:   "93598ed2d47fe2dd08bb4e1aa9b9ab7c338d927bf38fa52aa7c85d16364d3e0e",
         intel: "2bbc31b0e04f177e5eb8eace61300de45be1399204b4f39f308ba500fc0ba805"

  # Homebrew's vocabulary is arm/intel; the release names the files after the
  # Rust target triple, so the mapping is explicit. Without it the cask asks
  # for ncrs-arm-apple-darwin.tar.gz and 404s on every machine.
  arch arm: "aarch64", intel: "x86_64"

  url "https://github.com/CosmicDriftGameStudio/ncrs/releases/download/v#{version}/ncrs-#{arch}-apple-darwin.tar.gz",
      verified: "github.com/CosmicDriftGameStudio/ncrs/"
  name "ncrs"
  desc "Norton Commander style dual-panel file manager"
  homepage "https://github.com/CosmicDriftGameStudio/ncrs"

  livecheck do
    url :url
    strategy :github_latest
  end

  binary "ncrs-#{arch}-apple-darwin/ncrs"

  # No zap stanza on purpose. The app writes no preferences, no caches and no
  # support files, so there is nothing `brew uninstall` would leave behind. The
  # install.sh path installs into ~/.local/bin and is not managed by Homebrew;
  # mixing the two is the user's choice, and `brew uninstall --cask ncrs`
  # removes only what Homebrew installed.
end
