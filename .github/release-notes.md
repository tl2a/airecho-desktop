## Downloads

| | Apple Silicon / ARM | Intel / AMD |
|---|---|---|
| **macOS 15 or later** | `Kopico_Desktop-*-arm64.dmg` | `Kopico_Desktop-*-x86_64.dmg` |
| **Linux** (glibc 2.43 or later, e.g. Ubuntu 26.04, Fedora 44) | `Kopico_Desktop-*-aarch64.AppImage` | `Kopico_Desktop-*-x86_64.AppImage` |

**Requirement:** [croc](https://github.com/schollz/croc) must be installed (`brew install croc`,
`port install croc`, or your distribution's package / `curl https://getcroc.schollz.com | bash`).
Kopico runs it to send and receive.

**macOS:** the app is not notarized. Drag it to Applications; on first launch macOS blocks
it. Open **System Settings → Privacy & Security** and click **Open Anyway** (or run
`xattr -dr com.apple.quarantine "/Applications/Kopico Desktop.app"`). The very first start
takes a little longer while macOS checks the bundled libraries.

**Linux:** `chmod +x Kopico_Desktop-*.AppImage` and run it. The *Send with Kopico*
file-manager entries aren't part of the AppImage.

Checksums: `SHA256SUMS`.
