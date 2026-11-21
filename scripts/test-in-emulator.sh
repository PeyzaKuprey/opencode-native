#!/usr/bin/env bash
set -euo pipefail
ADB=${ADB:-/home/noname/.local/bin/adb}
APK=${APK:-/home/noname/emul-work/termux.apk}
BIN=${BIN:-/home/noname/my-project/opencode-native/target/x86_64-linux-android/release/opencode-native}
DEB=${DEB:-/home/noname/my-project/opencode-native/dist/opencode-native_0.1.0_x86_64.deb}
SERIAL=${SERIAL:-emulator-5554}

$ADB -s "$SERIAL" wait-for-device
for i in $(seq 1 120); do
  B=$($ADB -s "$SERIAL" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')
  [ "$B" = "1" ] && break
  sleep 2
done
$ADB -s "$SERIAL" install -r "$APK" || true
$ADB -s "$SERIAL" push "$BIN" /data/local/tmp/opencode-native
$ADB -s "$SERIAL" shell chmod 755 /data/local/tmp/opencode-native
$ADB -s "$SERIAL" shell 'run-as com.termux cp /data/local/tmp/opencode-native /data/data/com.termux/files/usr/bin/opencode-native && chmod 755 /data/data/com.termux/files/usr/bin/opencode-native'
$ADB -s "$SERIAL" shell 'run-as com.termux /data/data/com.termux/files/usr/bin/opencode-native --help'
