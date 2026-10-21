# Installing Locora

Instructions for an AI assistant (or a person) who installs Locora on a computer.

## What to install

- **Default: only the Locora CLI** (`locora`). Install it when the request just says "Locora",
  "the CLI" or gives no details.
- **The app as well**, only when the request asks for the app ("Locora app", "with the app",
  "everything").
- **Only the app**, when the request says so ("only the app", "without the CLI").

Every file comes from the latest release:
<https://github.com/wasser-quest/locora/releases/latest>. Check each download against
`SHA256SUMS.txt` from the same release before running it. Do not use other download sites.

## The CLI

1. Pick the archive for this system:

   | System | File |
   |---|---|
   | Linux x64 | `locora-cli_<version>_x86_64-unknown-linux-gnu.tar.gz` |
   | Linux ARM64 | `locora-cli_<version>_aarch64-unknown-linux-gnu.tar.gz` |
   | macOS Apple silicon | `locora-cli_<version>_aarch64-apple-darwin.tar.gz` |
   | macOS Intel | `locora-cli_<version>_x86_64-apple-darwin.tar.gz` |
   | Windows x64 | `locora-cli_<version>_x86_64-pc-windows-msvc.zip` |

2. Unpack it and put `locora` (Windows: `locora.exe`) in a folder on the PATH, for example
   `~/.local/bin` (create it and add it to the PATH if needed). No administrator rights are needed.
3. macOS only: the binary is not signed yet. Run `xattr -d com.apple.quarantine locora` once.
4. Check it: `locora --version`.
5. Read `locora guide` (the same text as [`llm.txt`](llm.txt)) before using Locora: it explains
   every command.

Without a matching archive, build it from source: install Rust with rustup, run
`rustup target add wasm32-wasip2`, clone this repository and run
`cargo install --path apps/cli --locked`.

## The app (only when asked for)

| System | File | How |
|---|---|---|
| Windows | `Locora_<version>_x64-setup.exe` (or `_x64_en-US.msi`) | Run it. It is not code-signed yet: *More info → Run anyway*. |
| macOS Apple silicon | `Locora_<version>_aarch64.dmg` | Open it, copy Locora to Applications, then run `xattr -dr com.apple.quarantine /Applications/Locora.app` once. |
| macOS Intel | `Locora_<version>_x64.dmg` | As above. |
| Debian, Ubuntu | `Locora_<version>_amd64.deb` | `sudo apt install ./Locora_<version>_amd64.deb` |
| Fedora, openSUSE | `Locora-<version>-1.x86_64.rpm` | `sudo dnf install ./Locora-<version>-1.x86_64.rpm` |
| Other Linux | `Locora_<version>_amd64.AppImage` | `chmod +x` it and run it. |

Phones and tablets cannot be set up from a computer. Tell the person to open the release page on
the device: Android takes `Locora_<version>_android-arm64.apk`; iPhone and iPad take the unsigned
`Locora_<version>_ios-unsigned.ipa`, installed with AltStore or Sideloadly. Or use the web app at
<https://app.locora.cloud>, which needs no installation and works offline.

## When you are done

Tell the person what you installed, the version (`locora --version`, or the app's file name), and
how to start it (`locora tutorial` is a good first command; the app is in the start menu or
Applications).
