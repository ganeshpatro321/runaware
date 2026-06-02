class Runaware < Formula
  desc "Local runtime awareness for AI coding agents"
  homepage "https://github.com/ganeshpatro321/runaware"
  version "0.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ganeshpatro321/runaware/releases/download/v#{version}/runaware-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_AARCH64_APPLE_DARWIN_SHA256"
    else
      url "https://github.com/ganeshpatro321/runaware/releases/download/v#{version}/runaware-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_X86_64_APPLE_DARWIN_SHA256"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/ganeshpatro321/runaware/releases/download/v#{version}/runaware-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_X86_64_LINUX_GNU_SHA256"
    else
      odie "RunAware Homebrew formula currently supports Linux x86_64 only"
    end
  end

  def install
    bin.install "runaware"
  end

  test do
    assert_match "RunAware data directory", shell_output("#{bin}/runaware doctor")
  end
end
