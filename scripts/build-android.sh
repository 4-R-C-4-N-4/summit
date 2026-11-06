#!/usr/bin/env bash
# Cross-compile summitd + summit-ctl for Android (no Android Studio needed).
#
# Needs only the NDK. Set ANDROID_NDK_HOME, or have it at ~/Android/android-ndk-*.
#   rustup target add aarch64-linux-android
#   scripts/build-android.sh            # arm64 phones (default)
#   ANDROID_TARGET=x86_64-linux-android scripts/build-android.sh   # emulator
#
# Output: ${CARGO_TARGET_DIR:-target}/<triple>/release/{summitd,summit-ctl}
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET="${ANDROID_TARGET:-aarch64-linux-android}"
API="${ANDROID_API:-26}"

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    ANDROID_NDK_HOME="$(ls -d "$HOME"/Android/android-ndk-* "$HOME"/Android/Sdk/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
fi
if [[ ! -d "${ANDROID_NDK_HOME:-}" ]]; then
    echo "NDK not found. Set ANDROID_NDK_HOME." >&2
    exit 1
fi

host_tag="$(uname -s | tr '[:upper:]' '[:lower:]')-x86_64"
bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host_tag/bin"
cc="$bin/${TARGET}${API}-clang"
[[ -x "$cc" ]] || { echo "missing $cc" >&2; exit 1; }

triple_env="$(echo "$TARGET" | tr '[:lower:]-' '[:upper:]_')"
export "CC_${TARGET//-/_}=$cc"
export "AR_${TARGET//-/_}=$bin/llvm-ar"
export "CARGO_TARGET_${triple_env}_LINKER=$cc"

cargo build --release --target "$TARGET" -p summitd -p summit-ctl
out="${CARGO_TARGET_DIR:-target}/$TARGET/release"
"$bin/llvm-strip" "$out/summitd" "$out/summit-ctl"
ls -lh "$out/summitd" "$out/summit-ctl"
