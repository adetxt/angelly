#!/usr/bin/env bash
# Type-check the app for another OS from a macOS or Linux host, using `zig cc` as the C
# toolchain (needed by build scripts such as ring's and gpui's).
#
# Usage: scripts/cross-check.sh linux|windows [cargo check args]
#
# Requires zig and the rust target (rustup target add x86_64-unknown-linux-gnu or
# x86_64-pc-windows-gnu). libghostty-vt is replaced by an empty archive because `check`
# never links. The Windows run stops in the `terminal` crate, which still uses
# Unix-only pty calls; everything compiled before that point is the dependency tree.
set -euo pipefail

os=${1:?usage: cross-check.sh linux|windows [cargo check args]}
shift
case $os in
  linux) triple=x86_64-unknown-linux-gnu; zig=x86_64-linux-gnu ;;
  windows) triple=x86_64-pc-windows-gnu; zig=x86_64-windows-gnu ;;
  *) echo "unknown os: $os" >&2; exit 2 ;;
esac

rust=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# cc-rs passes --target=<rust triple>, which zig does not understand.
for tool in cc c++ ar; do
  case $tool in ar) exec_line='exec zig ar "$@"' ;; *) exec_line="exec zig $tool -target $zig \"\$@\"" ;; esac
  cat > "$work/zig$tool" <<SCRIPT
#!/bin/sh
for arg in "\$@"; do
  shift
  case "\$arg" in --target=*) ;; *) set -- "\$@" "\$arg" ;; esac
done
$exec_line
SCRIPT
  chmod +x "$work/zig$tool"
done

# gpui's Windows build script wants a resource compiler; `check` does not use the output.
cat > "$work/rc-stub" <<'SCRIPT'
#!/bin/sh
exit 0
SCRIPT
chmod +x "$work/rc-stub"
cp "$work/rc-stub" "$work/llvm-rc"
cp "$work/rc-stub" "$work/x86_64-w64-mingw32-windres"

mkdir -p "$work/vt/lib"
: > "$work/vt/lib/libghostty-vt.a"
: > "$work/vt/lib/ghostty-vt-static.lib"

key=${triple//-/_}
export PATH="$work:$PATH"
export "CC_$key=$work/zigcc" "CXX_$key=$work/zigc++" "AR_$key=$work/zigar"
export GHOSTTY_VT_PREFIX="$work/vt"
export RUST_FONTCONFIG_DLOPEN=1
cd "$rust"
cargo check -p angelly --target "$triple" "$@"
