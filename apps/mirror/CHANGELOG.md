# Changelog

Changes to `govee-mirror-pc`, the Windows app in `apps/mirror`. It versions
apart from `govee-toolkit` and releases under `mirror-vX.Y.Z`, as a GitHub
release that carries `govee-mirror-pc.exe`. The policy is
[`../../docs/versioning.md`](../../docs/versioning.md).

## [0.2.0] — 2026-11-22

### Added

- An icon in the notification area. A click shows the window. Its menu
  shows the window, starts and stops the mirror, turns start with Windows
  on and off, and exits the app.
- Closing the window hides it to the notification area, and the mirror
  carries on. Exit in the icon's menu ends the app.
- Start with Windows starts the app in the notification area.
- The window and the icon show a color wheel.
- The Razer Wired ARGB Controller (Razer Chroma Addressable RGB Controller)
  over USB, with Synapse closed. Each port with LEDs is a light, and each
  LED a segment. The LED counts per port are a setting, and Exit hands the
  LEDs back to the controller's own effect.
- Profiles: named setups, each with an optional program. While that program
  runs, the app switches to its profile, and it switches back when the
  program ends. The process list is read every 5 seconds, and only while a
  profile names a program.
- One cell feeds a segment of each light that paints it.
- A box fill: draw a box and type a range, such as 1 to 60. With more
  segments than columns or rows, each cell drives a run of segments and the
  range is covered with no gap. A range from high to low runs the other way.
- A segment count set by hand for a Razer port, and a blend between its
  segments.

### Changed

- The settings file holds the profiles. A file of the earlier format becomes
  the "Default" profile.

## [0.1.1] — 2026-11-22

### Changed

- The app is `govee-mirror-pc`, and the executable `govee-mirror-pc.exe`. A
  start-with-Windows entry written under the earlier name moves to the new
  executable on the first start.
- The executable links the C runtime statically. It starts on a Windows
  machine with no Visual C++ Redistributable installed.

## [0.1.0] — 2026-11-21

### Added

- Mirror one screen onto Govee lights over `lan`. A grid of square cells
  covers the screen, and each cell feeds one segment of one light. A segment
  that several cells feed takes their average.
- Capture through DXGI Desktop Duplication. The GPU shrinks each frame, and
  a copy about 256 pixels wide comes back to the CPU.
- A light list with power, brightness and color, and a palette of numbered
  segment swatches. A click on a swatch blinks that segment on the light.
- A segment count per light: a resolution that the device file offers, or a
  count set by hand.
- Mirror on launch, and start with Windows, minimized.
- A saturation setting that moves averaged colors away from grey, and a
  per-light switch that blends between segments on the light.
- A settings file that does not parse is moved to `mirror.json.bad` rather
  than overwritten, and a crash writes `mirror-crash.log`.
- Patterns that the lights mirror in place of the screen: rainbow rings, a
  color wheel of 12 solid wedges, and a spiral of the same wedges. Each one
  moves or holds still.
