# Changelog

Changes to `govee-mirror-pc`, the Windows app in `apps/mirror`. It versions
apart from `govee-toolkit` and releases under `mirror-vX.Y.Z`, as a GitHub
release that carries `govee-mirror-pc.exe`. The policy is
[`../../docs/versioning.md`](../../docs/versioning.md).

## [Unreleased]

### Added

- **Key and LED effects** for the Razer Huntsman V2 keyboard and the Razer
  Strider Chroma mousepad, each in a card of its own in the Razer section.
  Put keys or LEDs in groups on a drawing of the device, and give each group
  Static, Breathing, Spectrum, Wave or Starlight, with a color and a speed.
  Those in no group are dark. Each profile holds its own effects.
- **Off** for the keyboard and the mousepad: every key or LED dark. With
  **Own effect** or when the app exits, the device gets back the effect that
  it keeps in its own memory.
- **Macro keys**: each of the three macro keys of the Huntsman V2 can switch
  to a profile. The app holds the keyboard in driver mode while a key has a
  profile, and hands it back when it exits.

### Changed

- The profile row moves from above the grid to under the top bar.
- Less CPU while the window is in the tray: the window draws nothing there,
  where it drew every frame before.
- The sound capture pauses while neither the mirror nor the window reads it,
  and reads the output every 20 ms instead of every 10 ms.
- The Razer devices share one list of the USB devices, refreshed every 10 s
  at most. One list takes about half a second on a PC with many devices.
- The screen capture shrinks each frame in one shader pass, in place of a
  copy of the screen and a mip chain. On a 4K frame the GPU time went from
  0.24 ms to 0.09 ms.

## [0.5.0] — 2026-11-23

### Added

- **Saved lights**: the app keeps the address of each Govee light that a
  scan finds, with its name and SKU, in `mirror.json`. Each later scan asks
  those addresses directly, so a light that the multicast scan misses still
  comes back. A light that moves to a new address replaces the old one.
- The list in Govee **General** shows each saved light, marks the ones that
  the last scan did not reach, and has **Forget** to stop asking one. The
  settings of a light stay keyed by its MAC, so a light that comes back
  keeps them.

## [0.4.0] — 2026-11-23

### Added

- A card for the Razer BlackShark V3 Pro headset in the Razer section. Its
  switch makes Windows play sound on the headset while it is on, and on a
  picked output while it is off. The app reads the connection from the USB
  dongle, so it acts at the next time the headset turns on or off.
- A Windows notification at each headset switch, "Headset on" or "Headset
  off", with the output that now plays. A checkbox in the headset card turns
  it off.
- A brightness slider on each Razer port, and one for every port in Razer
  **General**. It scales the colors that the mirror sends.
- **Sound reaction**, under the screen: a spectrum of the default output,
  and beside it each light with **Reacts**, **At rest** and **On bass**. A
  hit raises each reacting light to its **On bass** level, and the light
  falls back over the **Fade** time. **Adaptive** finds a hit as a jump above
  the recent average, with a **Sensitivity**; **Fixed line** uses a level
  dragged on the spectrum. The hit is read from the kick drum (40–100 Hz) or
  from every band below 150 Hz. A **SOUND** tag on the card marks each light
  that reacts.
- **Light addresses**: an IP box and **Add** beside **Scan**, and the list in
  Govee **General**. Each scan also asks these addresses directly, and every
  light it already knows, so a network that drops the multicast scan still
  finds them.

### Changed

- The window and the tray show the name "Govee Razer Mirror Windows 11 PC".
  The executable stays `govee-mirror-pc.exe`.

### Removed

- The frames-sent count on each light card.

## [0.3.0] — 2026-11-22

### Added

- Under the start button, the frame rate that the mirror reaches, and the
  share of CPU and GPU that the app uses, as Task Manager counts them. The
  app reads the share once a second, only while the window shows.
- Each light card has On and Off buttons in its header, and a dot: green
  while the light takes frames, red after a failure.
- The light list groups the lights by brand, each brand in a section with
  a colored header. A folded "General" block in each section holds what
  covers the whole brand: for Govee, power, color and brightness for every
  light at once; for Razer, the LED count of each port of the ARGB
  controller.

### Changed

- The top bar is one row: the start button, then titled groups for the
  screen, the output sliders, the source and the startup options.
- Each slider shows its name and its value above a filled track: the frame
  rate in fps, the smoothing in percent, the saturation as the boost over
  the screen ("Off" at none), and the pattern speed as a factor. The frame
  rate is a slider.
- The profile row sits above the grid.
- A light card is collapsed by default. Its header shows the name, and the
  card opens to show its segments, color and brightness.
- The screen capture runs only when something reads it. The mirror on the
  screen gets the rate set in the settings. The window preview alone gets 10
  frames per second. With the mirror stopped or on a pattern, and the window
  hidden, the capture stops and releases the screen.
- With one segment per LED, the blend of a Razer port gives each LED the
  mean of its color and the colors of the LEDs next to it.

### Fixed

- The controls on a light's card take their clicks. The card took the click
  for itself, so a checkbox, a list or a button there did nothing.
- A click on a light's name chooses the light. The name took the click to
  select its text.

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
