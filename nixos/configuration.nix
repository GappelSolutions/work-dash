{ config, pkgs, lib, ... }:

let
  # Bump this on every rebuild you intend to flash — it lands in the output
  # filename (sdImage.imageBaseName below) so a stale image in
  # nixos/result/sd-image/ or on the Windows side is never mistaken for the
  # latest one.
  imageVersion = "8";

  # Wifi confirmed working on real hardware (v6 debug pass) — kiosk back on
  # for the real validation run.
  enableKiosk = true;

  # Outside the repo entirely (not a flake-relative `./...` path) so it's
  # never subject to the flake's git-tracked-source purity check — reading
  # it needs `--impure` (see nixos/README.md for the required pre-build
  # decrypt step and the exact build command).
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

  # Decrypted once at build time (not on the Pi — see nixos/README.md) from
  # ./secrets/work-dash-pi.env.age into ~/.work-dash-pi-secrets/, which this
  # config then bakes into the image. `set -a` exports every var the file
  # defines into the kiosk command's environment. Only whoever holds the
  # private key in ./secrets/secrets.nix's recipient list can produce the
  # plaintext, so only they can build a working image — no key material of
  # any kind ends up in git or on the Pi itself.
  #
  # `environment.etc.source` with a raw path outside the store does NOT copy
  # the content in — it just symlinks to that literal path, which only
  # exists on the builder's machine. On the Pi that symlink is dangling, so
  # the env vars silently never load. `writeText`+`readFile` forces the
  # actual content into the store so it lands in the image.
  envFile = "/etc/work-dash/env";
  workDashEnvContent = pkgs.writeText "work-dash-env"
    (builtins.readFile (secretsDir + "/work-dash-pi.env.plain"));

in
{
  ###########################################################################
  # Secrets — baked in at build time (see nixos/README.md for the required
  # pre-build decrypt step and `--impure` build flag); nothing to configure
  # on the Pi itself.
  ###########################################################################

  environment.etc."work-dash/env" = {
    source = workDashEnvContent;
    # 0600 root-only was unreadable by the "kiosk" user the greetd session
    # runs as — `source` failed silently (no `set -e`), so the vars never
    # loaded even after the symlink fix above. Moot to restrict further
    # anyway: the underlying /nix/store path is world-readable regardless
    # of this mode, same tradeoff already accepted for workDashEnvContent.
    mode = "0444";
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

  # Panel is 800x480. Uncomment and set if it's mounted upside-down/sideways —
  # orientation is unknown until the panel is physically in its enclosure.
  # services.xserver.deviceSection = "" ; # (not used — no X server here)
  # See kiosk section below: `foot` inherits whatever rotation the compositor
  # reports; `cage` itself has no rotation flag, so panel rotation (if needed)
  # belongs in the KMS/DRM layer, e.g. a `video=DSI-1:panel_orientation=...`
  # kernel param — TODO once the panel's physical mount is known.

  ###########################################################################
  # Touch input
  ###########################################################################

  services.libinput.enable = true;

  ###########################################################################
  # Kiosk: greetd autologin -> cage (fullscreen Wayland compositor) -> foot
  # (truecolor terminal, forwards touch as mouse events) -> work-dash-client
  ###########################################################################

  users.users.kiosk = {
    isNormalUser = true;
    extraGroups = [ "video" "input" "seat" ];
  };

  ###########################################################################
  # Debug access — no SSH (device isn't networked yet, chicken-and-egg with
  # the wifi bug this image is meant to validate), so a physical-keyboard
  # login on another VT (Ctrl+Alt+F2 etc.) is the only way in. Without a
  # password no account can log in at all — NixOS locks accounts with no
  # hash by default, so this is required, not optional, for debugging.
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
          [ -f ${envFile} ] && source ${envFile}
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

  # wpa_supplicant reads WIFI_PSK via the ext: mechanism
  # (`secretsFile`/`pskRaw`, replacing the older `environmentFile`/`psk`+
  # "@VAR@" API). `secretsFile` ends up literally baked into
  # wpa_supplicant.conf as `ext_password_backend=file:<path>` and is read
  # from disk by wpa_supplicant at runtime *on the Pi* — a raw
  # `secretsDir + "/wifi.env.plain"` path only exists on the builder's
  # machine, so it must be a store path (via `writeText`) instead, same
  # issue as `workDashEnvContent` above.
  networking.wireless = {
    enable = true;
    secretsFile = pkgs.writeText "wifi-secrets"
      (builtins.readFile (secretsDir + "/wifi.env.plain"));
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
}
