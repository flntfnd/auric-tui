class Auric < Formula
  desc "Cross-platform terminal audio player built in Rust"
  homepage "https://github.com/flntfnd/auric-tui"
  version "0.1.0"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/flntfnd/auric-tui/releases/download/v#{version}/auric-aarch64-apple-darwin.tar.gz"
      # sha256 will be filled after first release
    end
    on_intel do
      url "https://github.com/flntfnd/auric-tui/releases/download/v#{version}/auric-x86_64-apple-darwin.tar.gz"
    end
  end

  on_linux do
    url "https://github.com/flntfnd/auric-tui/releases/download/v#{version}/auric-x86_64-unknown-linux-gnu.tar.gz"
  end

  def install
    bin.install "auric"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/auric --version")
  end
end
