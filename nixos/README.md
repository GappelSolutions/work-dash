# nixos/ — Pi 4B dashboard kiosk

Declarative NixOS config for the wall-mounted Raspberry Pi 4B: 7" DSI touch
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

**Confirmed** (via `nix eval github:nvmd/nixos-raspberrypi#nixosModules
--apply builtins.attrNames`, and reading the flake's own source):
- Flake input, `nixos-raspberrypi.lib.nixosSystem` helper
- Module attribute paths are a nested attrset:
  `nixosModules.raspberry-pi-4.{base,bluetooth,case-argonone,display-vc4}` —
  not flat dotted-string keys (an earlier version of this file guessed the
  latter and was wrong; verified once and worth restating since the same
  mistake is easy to repeat for other boards/module names).
- No RP1 chip on Pi 4B (that's Pi 5-only) — DSI comes straight off the VC4
  GPU. `display-vc4` only adds an Xorg `PrimaryGPU` OutputClass, which is
  irrelevant under cage/Wayland (no X server here), so it isn't imported.
- `boot.loader.raspberry-pi.bootloader` defaults to `"uboot"` for Pi 4B
  (nixos-raspberrypi's `modules/raspberry-pi-4.nix`) — left at its default,
  not overridden.
- `packages.aarch64-linux.sdImage` needs `modules/installer/sd-card/
  sd-image-raspberrypi.nix` imported explicitly — `raspberry-pi-4.base`
  alone does not wire up `config.system.build.sdImage` (that's opt-in, only
  added by nixos-raspberrypi's own `*-installer` configs).
- Cross-building `aarch64-linux` from an `x86_64-linux` builder needs qemu
  binfmt registered (`boot.binfmt.emulatedSystems = [ "aarch64-linux" ]`
  on the *building* machine's own NixOS config) — without it, `nix build`
  silently schedules the build and then fails every derivation with a
  content-free "1 dependency failed" cascade and no actual build log, which
  is confusing to debug from the error output alone.

**Not confirmed — flagged inline in `configuration.nix` as TODOs, fix these
on first boot:**
- Whether `hardware.raspberry-pi.config.pi4` (vs. some other variant-keyed
  attribute name) is the right path into config.txt on this specific flake
  — `hardware.raspberry-pi.config.all.options` is confirmed to exist
  (`modules/raspberrypi.nix`), but the per-board `.pi4`/`.pi5` sections
  weren't traced further.
- The **panel init / touch overlay name** — this depends on which vendor
  actually made the "7-Zoll-DSI-Touchscreen" panel. `vc4-kms-dsi-7inch` (the
  overlay for the official Raspberry Pi Foundation 7" touchscreen, widely
  reused by compatible third-party clones) is a reasonable first guess, not
  a confirmed match for this particular panel. Many of these panels
  auto-init via the DSI EEPROM and need nothing extra; if the screen stays
  blank or touch doesn't register, check `dmesg | grep -iE 'drm|i2c|touch'`
  and look for the vendor's actual overlay name.
- **Physical mounting orientation** (rotation) — unknown until the panel is
  in its enclosure. See the commented block in `configuration.nix`.

## Setup — flash and boot, no on-device steps

Everything (server API key, wifi PSK) is baked into the image at **build**
time, not decrypted on the Pi. Only whoever holds the private key listed in
`secrets/secrets.nix` can produce a working image — nothing decrypts at
runtime, so there's no separate per-device identity and no manual step on
the Pi itself.

1. One-time per builder: decrypt the two agenix secrets to
   `~/.work-dash-pi-secrets/` (outside the repo — deliberately not a
   `./secrets/...` flake-relative path, so it's never subject to the
   flake's git-tracked-source purity check and can never accidentally be
   committed):
   ```sh
   mkdir -p ~/.work-dash-pi-secrets
   agenix -d secrets/work-dash-pi.env.age -i ~/.ssh/id_ed25519 \
     > ~/.work-dash-pi-secrets/work-dash-pi.env.plain
   agenix -d secrets/wifi.env.age -i ~/.ssh/id_ed25519 \
     > ~/.work-dash-pi-secrets/wifi.env.plain
   chmod 600 ~/.work-dash-pi-secrets/*.plain
   ```
   (`-i` must be a private key whose matching public key is in
   `secrets/secrets.nix`'s recipient list — add your own there and re-run
   `agenix -r` if you need to build from a different machine.)
2. Build the SD image (`--impure` is required — that's what lets
   `configuration.nix` read the plaintext files above):
   ```sh
   nix build --impure --accept-flake-config .#packages.aarch64-linux.sdImage
   # result/sd-image/*.img.zst — decompress and dd/Raspberry Pi Imager it
   # onto the SD card.
   ```
3. Fill in the remaining hardware TODOs above (display overlay, orientation)
   once the panel's in its enclosure and you can see boot output.
4. Boot — the Pi connects to wifi (`Semax_Gast`), comes up straight into
   the kiosk fullscreen, and shows today's board from
   `https://workdash.gappel.com`. If the server's unreachable, the clock
   page shows a red "SERVER UNREACHABLE" banner instead of failing silently
   (`client/src/ui/clock.rs`).

For an already-installed system reachable over SSH instead of reflashing:
```sh
nixos-rebuild switch --impure --flake .#dashboard --target-host root@<pi-ip>
```

### Flashing from Windows when the image was built in WSL

The build runs in WSL (`nix build` needs the Nix daemon); the SD card reader
on this machine is only reachable from Windows. Two quirks make the naive
approach fail, plus a fix for a write error that isn't about the image at
all.

**Getting the file out of WSL**: `\\wsl$\<Distro>\...\result\sd-image\...`
does not resolve, even though `result` itself does — Windows' 9P share can't
traverse through the Nix store symlink `result` points into (confirmed:
walking the path component-by-component, everything up to and including
`result` resolves, `result\sd-image` does not, repeatably). Copy from
*inside* WSL to `/mnt/c` instead, which sidesteps the share entirely:
```sh
wsl -d NixOS -- cp -L ~/dev/private/work-dash/nixos/result/sd-image/*.img.zst /mnt/c/temp/
wsl -d NixOS -- zstd -d -T0 -f /mnt/c/temp/<name>.img.zst -o /mnt/c/temp/<name>.img
```
(`zstd` is already on `PATH` inside the NixOS closure — no need to install
anything on the Windows side.)

**The SD reader is PCIe, not USB**: on this machine it enumerates as
"Realtek PCIE CardReader" (SCSI bus in `Get-Disk`), not a USB mass-storage
device — so `usbipd-win` cannot pass it through to WSL (there's no USB busid
for it to bind). Flashing has to happen from Windows tooling (Raspberry Pi
Imager), not `dd` inside WSL, unless an external USB reader is used instead.
Identify the right disk with `Get-Disk` before ever pointing a flash tool at
it — it's consistently ~128GB/127865454592 bytes; the internal NVMe system
disk is a completely different size and must never be the target.

**"Write protected" / "in use by another app" in rpi-imager, even though
the disk and adapter both look fine**: this happened on two separate builds
in a row and the same fix cleared it both times. In order of what to try
(check with `Get-Disk`/`Get-Partition` between each step, don't skip to the
last one blind):
1. `Get-Disk` may show `IsReadOnly: True` — clear it with elevated
   `Set-Disk -Number <N> -IsReadOnly $false`. If it silently stays `True`
   after that, it's a real hardware write-protect signal — check the
   physical lock tab on the microSD-to-SD adapter (slide toward the
   contact-pin end to unlock) and make sure the microSD is fully seated in
   the adapter first.
2. A stale drive-letter mount can hold a lock even after the above:
   `Get-Partition -DiskNumber <N> -PartitionNumber 1 | Remove-PartitionAccessPath -AccessPath "D:\"`
   (check `Get-Partition`'s actual `AccessPaths`/`DriveLetter` first — this
   errors "access path not valid" if the letter's already gone).
3. If both of the above are already clean and rpi-imager — relaunched as
   Administrator — still fails identically: elevated `diskpart` →
   `select disk <N>` → `clean`. This wipes the partition table, which is
   harmless here since the entire point is to overwrite the card. This is
   what actually resolved it both times, after steps 1–2 were already true.
   Retry the write in rpi-imager immediately after.
   (`Set-Disk -IsOffline $true` does not work as an alternative to `clean`
   — removable media rejects it outright with "Not Supported".)

Elevation note: `Set-Disk`/`diskpart` need a real elevated PowerShell
window — if you're driving this through an agent/automation session, UAC
prompts triggered from that session may not render at all (silent failure
or "operation canceled by user"); run these commands yourself in your own
elevated window.

**If C: runs low on space from the WSL build**: the NixOS distro's
`ext4.vhdx` only grows, never auto-shrinks, and can balloon to hundreds of
GB even when actual usage inside is much smaller (seen: ~806GB on disk vs.
~187GB actually used). `wsl --manage NixOS --set-sparse true` is blocked by
default on this WSL version ("potential data corruption", needs
`--allow-unsafe` — not worth forcing). What works: shut down WSL fully
(`wsl --shutdown`), then `wsl --manage NixOS --resize 512GB` to shrink the
filesystem — but this alone does **not** shrink the actual `.vhdx` file on
disk, it only resizes the ext4 filesystem inside it. Follow it with an
elevated `diskpart`: `select vdisk file="<path-to>\ext4.vhdx"` →
`attach vdisk readonly` → `compact vdisk` → `detach vdisk`, which is the
step that actually reclaims the space on the Windows side (the vhdx path is
under `%LOCALAPPDATA%\wsl\<GUID>\ext4.vhdx`; the GUID is in
`HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss`). 512GB is now this
distro's cap.

## Files

- `flake.nix` — inputs (`nixpkgs`, `nixos-raspberrypi`), the
  `nixosConfigurations.dashboard` output, and `packages.aarch64-linux.sdImage`
  (wires in `nixos-raspberrypi`'s `sd-image-raspberrypi.nix` for
  `config.system.build.sdImage`).
- `configuration.nix` — display/touch setup, the `work-dash-client` Nix
  package (built from this repo's workspace), the `greetd` → `cage` →
  `foot` kiosk session, wifi, and the build-time secrets wiring.
- `secrets/secrets.nix` — agenix recipient list (who can decrypt/build).
- `secrets/work-dash-pi.env.age`, `secrets/wifi.env.age` — encrypted
  `WORK_DASH_SERVER_URL`/`WORK_DASH_API_KEY` and `WIFI_PSK`. Edit with
  `agenix -e <file> -i <your private key>` from `secrets/`; re-encrypt for a
  new recipient with `agenix -r -i <a-key-already-in-secrets.nix>`.
