# nixos/ — Pi 5 dashboard kiosk

Declarative NixOS config for the wall-mounted Raspberry Pi 5: 7" DSI touch
display, boots straight into the `work-dash-client` TUI, no desktop.

## Why cage + foot, not bare console or X

The kernel's own text console (a plain `getty` autologin) only remaps 16
fixed color slots and has no usable touch support (touch needs a proper
input stack — `libinput` — which only Wayland/X compositors drive). X +
a window manager works but is a lot of moving parts for a 7" always-on
panel. So: **`cage`** (a compositor that does nothing but run one app
fullscreen, with working `libinput` touch) running **`foot`** (a truecolor,
themable Wayland terminal that reports touch taps as mouse clicks) running
the existing `work-dash-client` binary. The client already handles mouse
clicks (`client/src/main.rs::on_mouse`) — a touch tap arrives as the same
event, no client changes needed.

## What's confirmed vs. what needs verifying on real hardware

**Confirmed** (from `nixos-raspberrypi`'s own README):
- Flake input, `nixos-raspberrypi.lib.nixosSystem` helper
- `raspberry-pi-5.display-rp1` is the right module for an RP1-connected
  MIPI DSI display
- `boot.loader.raspberry-pi.bootloader = "kernel"` is the recommended Pi 5
  bootloader setting

**Not confirmed — flagged inline in `flake.nix`/`configuration.nix` as
TODOs, fix these on first boot:**
- The exact attribute path for the module outputs in `flake.nix`
  (`nixosModules."raspberry-pi-5.display-rp1"` — dotted string key vs.
  nested attrset wasn't documented; run `nix flake show
  github:nvmd/nixos-raspberrypi` to check).
- Whether `hardware.raspberry-pi.config` (the config.txt-generating
  attrset, with `dt-overlays` / `base-dt-params`) exists on this specific
  flake — that schema is confirmed on the sibling `nix-community/
  raspberry-pi-nix` project, not verified here.
- The **panel init / touch overlay name** — this depends on which vendor
  actually made the "7-Zoll-DSI-Touchscreen" panel. Many of these auto-init
  via the DSI EEPROM and need nothing extra; if the screen stays blank or
  touch doesn't register, check `dmesg | grep -iE 'drm|i2c|touch'` and look
  for the vendor's overlay name. Do not assume the commented-out
  `vc4-kms-dsi-generic` placeholder in `configuration.nix` is correct —
  it's a guess at the shape of the fix, not a confirmed overlay name.
- **Physical mounting orientation** (rotation) — unknown until the panel is
  in its enclosure. See the commented block in `configuration.nix`.
- **Static LAN IP** — placeholder only, fill in for the real network.

## Setup

1. On the Pi (after first boot / before enabling the kiosk), create the
   secret env file the kiosk session sources — **not checked into this
   repo**:
   ```sh
   sudo install -Dm600 /dev/null /etc/work-dash/env
   printf 'WORK_DASH_SERVER_URL=http://<server-lan-ip>:<port>\nWORK_DASH_API_KEY=<pi-key>\n' \
     | sudo tee /etc/work-dash/env
   ```
2. Fill in the TODOs above (module attribute path, display overlay, static
   IP) in `flake.nix` / `configuration.nix`.
3. Build an installer image and flash it:
   ```sh
   nix --accept-flake-config build .#installerImages.rpi5
   # write the resulting image to an SD card / NVMe, as usual
   ```
   Or, for an already-installed system reachable over SSH:
   ```sh
   nixos-rebuild switch --flake .#dashboard --target-host root@<pi-ip>
   ```
4. Reboot — the Pi should come up straight into the kiosk, fullscreen,
   showing today's board from the server.

## Files

- `flake.nix` — inputs (`nixpkgs`, `nixos-raspberrypi`) and the
  `nixosConfigurations.dashboard` output.
- `configuration.nix` — display/touch setup, the `work-dash-client` Nix
  package (built from this repo's workspace), the `greetd` → `cage` →
  `foot` kiosk session, and networking placeholders.
