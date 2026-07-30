{ config, pkgs, lib, ... }:

let
  # Bump this on every rebuild you intend to flash — it lands in the output
  # filename (sdImage.imageBaseName below) so a stale image in
  # nixos/result/sd-image/ or on the Windows side is never mistaken for the
  # latest one.
  imageVersion = "9";

  # Wifi confirmed working on real hardware (v6 debug pass) — kiosk back on
  # for the real validation run.
  enableKiosk = true;

  # Outside the repo entirely (not a flake-relative `./...` path) so it's
  # never subject to the flake's git-tracked-source purity check — reading
  # it needs `--impure` (see nixos/README.md for the required pre-build
  # decrypt step and the exact build command). Only used for the device's
  # persistent age identity now (see sdImage.populateRootCommands below) —
  # everything else decrypts on-device via agenix at activation time, so
  # `system.autoUpgrade` can pull+apply new secrets with no rebuild-time
  # impurity and no access to the Pi itself.
  secretsDir = builtins.getEnv "HOME" + "/.work-dash-pi-secrets";

  # Packaged straight from this repo's Cargo workspace so the Pi image always
  # ships the exact client source checked in here — no separate build step or
  # prebuilt-binary handoff. Builds the whole workspace (client + server) via
  # `cargoLock.lockFile` but only compiles the `work-dash-client` member.
  workDashClient = pkgs.rustPlatform.buildRustPackage {
    pname = "work-dash-client";
    version = "0.1.0";
    src = ../.;
    cargoLock.lockFile = ../Cargo.lock;
    cargoBuildFlags = [ "-p" "work-dash-client" ];
    # Already covered by `cargo test` in CI/dev; skip rustPlatform's default
    # check phase so packaging this doesn't also try to build+test `server`.
    doCheck = false;
  };

  # The panel is touch-only (no physical mouse), but cage/wlroots always
  # renders a default xcursor image regardless of pointer-capable devices.
  # There's no cage flag to disable it, so the standard wlroots workaround is
  # a fully transparent xcursor theme, pointed to via XCURSOR_THEME below.
  invisibleCursorTheme = pkgs.runCommand "invisible-cursor-theme" {
    nativeBuildInputs = [ pkgs.xorg.xcursorgen pkgs.imagemagick ];
  } ''
    mkdir -p $out/share/icons/invisible/cursors
    convert -size 1x1 xc:none blank.png
    echo "1 0 0 blank.png" > blank.cfg
    xcursorgen blank.cfg $out/share/icons/invisible/cursors/default
    for name in left_ptr text pointer hand2 grabbing crosshair wait watch \
                move ns-resize ew-resize nesw-resize nwse-resize all-scroll; do
      ln -s default "$out/share/icons/invisible/cursors/$name"
    done
    mkdir -p $out/share/icons/invisible
    cat > $out/share/icons/invisible/index.theme <<EOF
    [Icon Theme]
    Name=invisible
EOF
  '';

  # foot is configured via an explicit --config file (see the kiosk command
  # below) instead of a NixOS/home-manager module — there's no upstream
  # `programs.foot` NixOS module, and this avoids depending on home-manager
  # just for one kiosk user's terminal palette.
  #
  # Palette echoes the existing ratatui client's named ANSI colors
  # (client/src/ui/{clock,kanban,menu,idle}.rs use Color::Cyan/Green/Yellow/
  # Red/Magenta/Gray/DarkGray/White) and the CMS's cyberdeck theme, so the
  # Pi and the browser CMS feel like the same product.
  footConfig = pkgs.writeText "work-dash-foot.ini" ''
    [main]
    term = xterm-256color
    font = JetBrainsMono:size=7
    pad = 0x0
    box-drawings-uses-font-glyphs = yes

    [cursor]
    style = block

    [colors]
    background = 0a0d11
    foreground = c7d2dd

    regular0 = 0d1218
    regular1 = ff5964
    regular2 = 59e0a0
    regular3 = ffb648
    regular4 = 4fc3f7
    regular5 = b388ff
    regular6 = 6fd6ff
    regular7 = c7d2dd

    bright0 = 3d4a5a
    bright1 = ff7a83
    bright2 = 8bffc9
    bright3 = ffd08a
    bright4 = 8fe0ff
    bright5 = d1b3ff
    bright6 = a6ecff
    bright7 = ffffff
  '';

  # The device's persistent age identity, generated once
  # (~/.work-dash-pi-secrets/device.key, git-ignored) and baked straight onto
  # the SD image's rootfs at build time via sdImage.populateRootCommands
  # below — NOT declared as environment.etc, deliberately, so ordinary
  # NixOS activation (including future `nixos-rebuild switch` runs
  # triggered by system.autoUpgrade) never touches or re-derives this path.
  # It's the one thing that only exists because of the initial flash;
  # everything else (env vars, wifi PSK) flows through agenix from here on
  # and can be updated over git with no on-device/SSH step.
  #
  # Must go through `writeText`+`readFile` (impure eval, not a raw path) —
  # the sd-image build runs in a sandbox with no access to arbitrary host
  # paths like `$HOME`; only real store paths are visible inside it. This
  # is the same fix `workDashEnvContent` needed before it was replaced by
  # agenix above.
  deviceKeyFile = pkgs.writeText "device-key"
    (builtins.readFile (secretsDir + "/device.key"));

in
{
  ###########################################################################
  # Secrets — encrypted files ship in git; agenix decrypts them at
  # activation time on the Pi itself using the device identity above, so
  # `system.autoUpgrade` can pick up secret changes with no rebuild-time
  # impurity and no access to the device. See nixos/README.md for the
  # one-time device-key generation + image-flash step.
  ###########################################################################

  age.identityPaths = [ "/etc/age/device.key" ];

  age.secrets.workDashEnv = {
    file = ./secrets/work-dash-pi.env.age;
    # World-readable: the "kiosk" user the greetd session runs as needs to
    # read this, and /run/agenix files default to 0400 root-only.
    mode = "0444";
  };

  age.secrets.wifiEnv = {
    file = ./secrets/wifi.env.age;
  };

  ###########################################################################
  # Boot / bootloader
  ###########################################################################

  # `boot.loader.raspberry-pi.bootloader` defaults to "uboot" for Pi 4B
  # (nixos-raspberrypi's `raspberry-pi-4.nix`) — that default is left as-is;
  # "kernel" is the newer generational bootloader but is only the
  # *recommended* default for Pi 5, not verified here for Pi 4.

  ###########################################################################
  # Display — 7in DSI touchscreen, 800x480
  ###########################################################################

  # TODO(verify on hardware): Pi 4B (unlike Pi 5) drives DSI straight off the
  # VC4 GPU, no RP1 chip involved — the *panel init sequence* and *touch
  # controller* are still vendor-specific to whichever 7" DSI panel this is
  # (commonly sold simply as "7-Zoll-DSI-Touchscreen für Raspberry Pi").
  # `vc4-kms-dsi-7inch` is the standard overlay for the official Raspberry Pi
  # Foundation 7" touchscreen and is widely reused by compatible third-party
  # clones — a reasonable first guess, not confirmed for this specific
  # panel. Whether `hardware.raspberry-pi.config.pi4` (vs. some other
  # variant-keyed attribute name) is the right path into config.txt on this
  # flake also isn't verified — check against real boot logs
  # (`dmesg | grep -i -e drm -e i2c -e touch`) on first boot.
  # hardware.raspberry-pi.config.pi4 = {
  #   dt-overlays = {
  #     vc4-kms-dsi-7inch.enable = lib.mkDefault true; # TODO: confirm this is the right panel
  #   };
  #   base-dt-params = {
  #     i2c_arm.enable = true; # touch controller is on I2C
  #   };
  # };

  hardware.i2c.enable = lib.mkDefault true;

  # Panel is 800x480, mounted upside-down. `cage` has no rotation flag itself
  # (wlroots draws whatever the DRM connector reports), so rotation belongs
  # at the KMS/DRM layer — the `panel_orientation` connector property, set
  # via this kernel param, flips console + Wayland output alike. Touch input
  # isn't remapped by this — see the udev calibration rule under "Touch
  # input" below.
  boot.kernelParams = [ "video=DSI-1:panel_orientation=upside_down" ];

  ###########################################################################
  # Touch input
  ###########################################################################

  services.libinput.enable = true;

  # Panel is rotated 180° at the KMS layer (see panel_orientation above), but
  # the touch controller still reports raw (non-rotated) coordinates, so taps
  # land diagonally opposite where they should. `services.libinput.touchpad.
  # calibrationMatrix` is NOT the fix here — that option only feeds the X11
  # `xf86-input-libinput` driver config (gated on `services.xserver.enable`),
  # and this kiosk is cage/wlroots on Wayland with no X server at all. libinput
  # itself (used directly by wlroots) reads calibration from the
  # `LIBINPUT_CALIBRATION_MATRIX` udev property instead, so it has to be set
  # via a udev rule. 6 numbers = first two rows of the 3x3 transform matrix
  # (row 3 is implicitly "0 0 1"); "-1 0 1 0 -1 1" is a 180° rotation.
  services.udev.extraRules = ''
    SUBSYSTEM=="input", ENV{ID_INPUT_TOUCHSCREEN}=="1", ENV{LIBINPUT_CALIBRATION_MATRIX}="-1 0 1 0 -1 1"
  '';

  ###########################################################################
  # Kiosk: greetd autologin -> cage (fullscreen Wayland compositor) -> foot
  # (truecolor terminal, forwards touch as mouse events) -> work-dash-client
  ###########################################################################

  users.users.kiosk = {
    isNormalUser = true;
    extraGroups = [ "video" "input" "seat" ];
  };

  ###########################################################################
  # Debug access — deliberately no SSH; the device sits on a public/guest
  # wifi network with no inbound reachability, so a physical-keyboard login
  # on another VT (Ctrl+Alt+F2 etc.) is the only way in. Without a password
  # no account can log in at all — NixOS locks accounts with no hash by
  # default, so this is required, not optional, for debugging.
  # `initialPassword` only seeds it on first activation (won't stomp a
  # password you later change on-device via `passwd`), plain root login for
  # now since this device has no other user worth separating privileges for.
  ###########################################################################
  users.users.root.initialPassword = "workdash-debug";

  services.greetd = {
    enable = enableKiosk;
    settings.default_session = {
      user = "kiosk";
      command = ''
        ${pkgs.bash}/bin/bash -c '
          set -a
          [ -f ${config.age.secrets.workDashEnv.path} ] && source ${config.age.secrets.workDashEnv.path}
          XCURSOR_THEME=invisible
          XCURSOR_PATH=${invisibleCursorTheme}/share/icons
          XCURSOR_SIZE=1
          set +a
          exec ${pkgs.cage}/bin/cage -s -- ${pkgs.foot}/bin/foot --config=${footConfig} -e ${workDashClient}/bin/work-dash-client
        '
      '';
    };
  };

  environment.systemPackages = [
    pkgs.cage
    pkgs.foot
    workDashClient
    # Debug tools for the physical-console validation pass (no SSH yet —
    # see the debug access section above): wpa_cli isn't on PATH by default
    # even with networking.wireless.enable, curl/iw for manually checking
    # server reachability and wifi association state.
    pkgs.wpa_supplicant
    pkgs.curl
    pkgs.iw
    pkgs.vim
  ];
  fonts.packages = [ pkgs.jetbrains-mono ];

  ###########################################################################
  # Networking — wifi, DHCP; the client talks to the public
  # https://workdash.gappel.com route (Caddy-fronted), not a LAN IP, so no
  # static address/gateway setup is needed here.
  ###########################################################################

  networking.hostName = "work-dash-pi";
  hardware.enableRedistributableFirmware = true; # Pi 4B wifi (brcmfmac) needs this

  # wpa_supplicant reads WIFI_PSK via the ext: mechanism (`secretsFile`/
  # `pskRaw`). `secretsFile` ends up literally baked into wpa_supplicant.conf
  # as `ext_password_backend=file:<path>` and is read from disk by
  # wpa_supplicant at runtime *on the Pi* — pointed at the agenix-decrypted
  # path (/run/agenix/wifiEnv), populated at activation time on-device.
  networking.wireless = {
    enable = true;
    secretsFile = config.age.secrets.wifiEnv.path;
    # `country` fixes brcmfmac "set chanspec ... fail reason -52" — with no
    # regulatory domain set, firmware defaults to a restrictive "world"
    # regdomain that rejects some channels outright.
    extraConfig = "country=CH";
    networks."Semax_Gast" = {
      pskRaw = "ext:WIFI_PSK";
      # Default authProtocols includes SAE (WPA3) — brcmfmac's SAE
      # external-auth offload is known-buggy on the Pi4's cyw43455 chip
      # ("external_auth failed status 15" = invalid pairwise cipher).
      # Force plain WPA2-PSK so it never attempts SAE at all.
      authProtocols = [ "WPA-PSK" ];
      # "Semax_Gast" is one band-steered SSID spanning 2.4/5/6 GHz across a
      # multi-AP deployment; the AP nearest this device apparently assigns
      # 5 GHz channel 140 (a DFS channel in the 120-140 UNII-2C range),
      # which the Pi 4's cyw43455/brcmfmac can't use at all (no DFS radar
      # detection support — every chanspec set on it fails with reason
      # -52, permanently, regardless of auth config). Restricting to
      # 2.4 GHz channels forces the AP to hand this device the 2.4 GHz
      # BSSID instead, sidestepping the band-steering decision entirely.
      extraConfig = ''
        freq_list=2412 2417 2422 2427 2432 2437 2442 2447 2452 2457 2462 2467 2472
      '';
    };
  };

  system.stateVersion = "25.11";

  time.timeZone = "Europe/Zurich";

  image.baseName = lib.mkForce "work-dash-pi-v${imageVersion}";

  ###########################################################################
  # One-time device-key bake — writes the persistent age identity straight
  # onto the SD image's rootfs at *image-build* time, outside the normal
  # Nix store/activation graph. Because nothing in this config declares
  # `/etc/age/device.key` via environment.etc, ordinary NixOS activation
  # (including every future `nixos-rebuild switch`, whether run manually or
  # by system.autoUpgrade below) never touches this path again after the
  # initial flash — it just persists on disk.
  ###########################################################################
  sdImage.populateRootCommands = ''
    mkdir -p ./files/etc/age
    install -m 0600 ${deviceKeyFile} ./files/etc/age/device.key
  '';

  ###########################################################################
  # Auto-update — polls the public flake repo daily, rebuilds+switches with
  # no on-device/SSH step. Secrets flow through agenix (see above) so this
  # also picks up secret changes, not just code/config. `--impure` is not
  # needed here: unlike the initial image build, nothing in the ongoing
  # rebuild reads outside the flake's git-tracked source anymore.
  ###########################################################################
  system.autoUpgrade = {
    enable = true;
    flake = "github:GappelSolutions/work-dash?dir=nixos#dashboard";
    flags = [ "--accept-flake-config" ]; # trusts the nixos-raspberrypi cachix substituter
    dates = "04:00";
    randomizedDelaySec = "30min";
    allowReboot = true;
    # Kiosk sits idle overnight; a reboot mid-shift would black the panel
    # out, so confine it to the same overnight window as the pull itself.
    rebootWindow = { lower = "03:00"; upper = "05:00"; };
  };
}
