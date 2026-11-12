#!/bin/bash
# Runs inside the angelly-linux-dev image: starts a software-rendered Wayland session
# (Weston on Xvfb), launches the app in it and writes screenshots to /out.
#
# Environment:
#   APP            executable to run (default /target/debug/angelly)
#   SHOT           screenshot base name (default angelly)
#   DELAY          seconds to wait before the first screenshot (default 25)
#   INPUT          text typed into the focused terminal, then Return, before the second
#                  screenshot (<SHOT>-input.png)
#   EXTRA_SHOTS    space separated xdotool key chords; each is pressed and followed by a
#                  screenshot named <SHOT>-<n>.png
set -u
mkdir -p -m 700 "$XDG_RUNTIME_DIR" "$HOME"
app=${APP:-/target/debug/angelly}
shot=${SHOT:-angelly}

Xvfb :98 -screen 0 1500x1000x24 >/dev/null 2>&1 &
sleep 1
export DISPLAY=:98
weston --backend=x11-backend.so --use-pixman --socket=wayland-0 --width=1400 --height=900 --debug >/tmp/weston.log 2>&1 &
sleep 3
export WAYLAND_DISPLAY=wayland-0
env -u DISPLAY "$app" >/out/"$shot".log 2>&1 &
pid=$!

capture() {
  (cd /tmp && rm -f wayland-screenshot-*.png && weston-screenshooter >/dev/null 2>&1 && mv wayland-screenshot-*.png "/out/$1.png")
}

sleep "${DELAY:-25}"
kill -0 "$pid" 2>/dev/null || { echo "app exited early"; tail -20 /out/"$shot".log; exit 1; }
capture "$shot"
if [ -n "${INPUT:-}" ]; then
  xdotool mousemove 700 500 click 1
  sleep 1
  xdotool type --delay 40 "$INPUT"
  xdotool key Return
  sleep 4
  capture "$shot-input"
fi
n=0
for chord in ${EXTRA_SHOTS:-}; do
  n=$((n + 1))
  xdotool key "$chord"
  sleep 3
  capture "$shot-$n"
done
kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
echo "screenshots written to /out"
