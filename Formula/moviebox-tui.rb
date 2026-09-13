class MovieboxTui < Formula
  VERSION = "0.1.19"
  MACOS_SHA256 = "cfa1b17b941d86945270b51f6565919ff9df19fa65e2e840b76cfc7682ffcc7a"
  LINUX_X64_SHA256 = "2e4666f4fc4c82f9e6812097d02e3762040b2b47e26c0b65f0fcb7c255eae949"
  LINUX_ARM64_SHA256 = "5d9b0dd7e30bcf22e7c30862454731253eeffd2f23b673ce4a80755b58c06ad7"

  desc "Stream movies, shows, anime, and live TV from your terminal"
  homepage "https://github.com/mesamirh/MovieBox-Tui"
  version VERSION
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    url "https://github.com/mesamirh/MovieBox-Tui/releases/download/v#{VERSION}/MovieBox_macOS_Universal.tar.gz"
    sha256 MACOS_SHA256
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/mesamirh/MovieBox-Tui/releases/download/v#{VERSION}/MovieBox_Linux_arm64.tar.gz"
      sha256 LINUX_ARM64_SHA256
    else
      url "https://github.com/mesamirh/MovieBox-Tui/releases/download/v#{VERSION}/MovieBox_Linux_x64.tar.gz"
      sha256 LINUX_X64_SHA256
    end
  end

  def install
    bin.install "moviebox-tui"
  end

  test do
    system "#{bin}/moviebox-tui", "--version"
  end
end
