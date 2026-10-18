#!/usr/bin/env bash
set -euo pipefail

app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$app_dir"
# Trunk accepts boolean values for NO_COLOR; some shells export it as 1.
if [[ -n "${NO_COLOR:-}" ]]; then
  export NO_COLOR=true
fi

fail() {
  printf 'Android build: %s\n' "$*" >&2
  exit 1
}

for tool in cargo rustup java javac sdkmanager trunk; do
  command -v "$tool" >/dev/null 2>&1 || fail "'$tool' fehlt. Siehe apps/app/README.md (Android)."
done
cargo tauri --version >/dev/null 2>&1 || fail "Tauri CLI fehlt: cargo install tauri-cli --version '=2.12.3' --locked"

if [[ -z "${JAVA_HOME:-}" ]]; then
  java_path="$(readlink -f "$(command -v java)")"
  export JAVA_HOME="$(dirname "$(dirname "$java_path")")"
fi
[[ -x "$JAVA_HOME/bin/java" && -x "$JAVA_HOME/bin/javac" ]] || fail "JAVA_HOME zeigt nicht auf ein JDK."

if [[ -z "${ANDROID_HOME:-}" ]]; then
  if [[ -d "$HOME/Android/Sdk" ]]; then
    export ANDROID_HOME="$HOME/Android/Sdk"
  else
    fail "ANDROID_HOME fehlt. Android SDK Command-line Tools installieren und ANDROID_HOME setzen."
  fi
fi
[[ -d "$ANDROID_HOME" ]] || fail "ANDROID_HOME ist kein Verzeichnis: $ANDROID_HOME"

if [[ -z "${NDK_HOME:-}" ]]; then
  export NDK_HOME="$ANDROID_HOME/ndk/29.0.13846066"
fi

rustup target add wasm32-unknown-unknown wasm32-wasip2 aarch64-linux-android
# Installing exact packages asks only for licenses they actually require.
sdkmanager --sdk_root="$ANDROID_HOME" "platform-tools" "platforms;android-37.0" "build-tools;37.0.0" "ndk;29.0.13846066"
[[ -d "$NDK_HOME" ]] || fail "NDK_HOME ist kein Verzeichnis: $NDK_HOME"
ndk_bins=("$NDK_HOME"/toolchains/llvm/prebuilt/*/bin)
[[ -d "${ndk_bins[0]}" ]] || fail "NDK LLVM-Toolchain fehlt in $NDK_HOME"
export CC_aarch64_linux_android="${ndk_bins[0]}/aarch64-linux-android24-clang"
export CXX_aarch64_linux_android="${ndk_bins[0]}/aarch64-linux-android24-clang++"
export AR_aarch64_linux_android="${ndk_bins[0]}/llvm-ar"
[[ -x "$CC_aarch64_linux_android" ]] || fail "Android ARM64-C-Compiler fehlt: $CC_aarch64_linux_android"

if [[ ! -d src-tauri/gen/android ]]; then
  cargo tauri android init --ci --skip-targets-install
fi
# Keep an existing generated project in sync with the versioned launcher icons.
cp -R src-tauri/icons/android/. src-tauri/gen/android/app/src/main/res/
cargo tauri android build --ci --debug --target aarch64 --apk
apk="$app_dir/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
[[ -s "$apk" ]] || fail "APK wurde nicht erzeugt: $apk"
apksigner="$ANDROID_HOME/build-tools/37.0.0/apksigner"
[[ -x "$apksigner" ]] || fail "Android apksigner fehlt: $apksigner"
"$apksigner" verify "$apk" || fail "APK ist beschädigt oder nicht gültig signiert: $apk"
printf 'APK: %s\n' "$apk"
