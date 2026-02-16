# To set up the tap, create repo copycatdb/homebrew-tap
# and copy this formula to Formula/lazypaw.rb

class Lazypaw < Formula
  desc "Instant REST API for SQL Server"
  homepage "https://github.com/copycatdb/lazypaw"
  license "MIT"
  version "0.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/copycatdb/lazypaw/releases/download/v#{version}/lazypaw-darwin-arm64.tar.gz"
      sha256 "PLACEHOLDER"
    else
      url "https://github.com/copycatdb/lazypaw/releases/download/v#{version}/lazypaw-darwin-x64.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/copycatdb/lazypaw/releases/download/v#{version}/lazypaw-linux-arm64.tar.gz"
      sha256 "PLACEHOLDER"
    else
      url "https://github.com/copycatdb/lazypaw/releases/download/v#{version}/lazypaw-linux-x64.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  def install
    bin.install "lazypaw"
  end

  test do
    assert_match "lazypaw", shell_output("#{bin}/lazypaw --help")
  end
end
