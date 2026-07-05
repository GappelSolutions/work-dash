{ config, pkgs, lib, ... }:

let
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
    font = JetBrainsMono:size=15
    pad = 12x12
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

  # Populated by the operator on the Pi itself (NOT checked into the flake —
  # this holds WORK_DASH_API_KEY, a secret). `set -a` exports every var the
  # file defines into the kiosk command's environment.
  #   sudo install -Dm600 /dev/null /etc/work-dash/env
  #   printf 'WORK_DASH_SERVER_URL=http://<server-lan-ip>:PORT\nWORK_DASH_API_KEY=<pi-key>\n' \
  #     | sudo tee /etc/work-dash/env
  envFile = "/etc/work-dash/env";

in
{
  ###########################################################################
  # Boot / bootloader
  ###########################################################################

  # Recommended for new Pi 5 installs per nixos-raspberrypi's README.
  boot.loader.raspberry-pi.bootloader = "kernel";

  ###########################################################################
  # Display — 7in DSI touchscreen, 800x480
  ###########################################################################

  # TODO(verify on hardware): the display-rp1 module (imported in flake.nix)
  # handles RP1-connected MIPI DSI output, but the *panel init sequence* and
  # the *touch controller* are vendor-specific to whichever 7" DSI panel this
  # is (commonly sold simply as "7-Zoll-DSI-Touchscreen für Raspberry Pi").
  # This block is the intent, not a confirmed-working overlay — the exact
  # dt-overlay name, and whether this flake even exposes config.txt via
  # `hardware.raspberry-pi.config` (that attrset shape is confirmed on the
  # sibling nix-community/raspberry-pi-nix flake, NOT verified here on
  # nixos-raspberrypi specifically), needs checking against real boot logs
  # (`dmesg | grep -i -e drm -e i2c -e touch`) on first boot.
  # hardware.raspberry-pi.config.pi5 = {
  #   dt-overlays = {
  #     # Many of these panels auto-init via the DSI EEPROM and need no
  #     # explicit overlay; if the panel stays blank, look for a
  #     # vendor-provided overlay name (often shipped as a .dtbo alongside
  #     # the vendor's Raspberry Pi OS image) and reference it here instead.
  #     vc4-kms-dsi-generic.enable = lib.mkDefault false; # TODO: confirm name
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

  services.greetd = {
    enable = true;
    settings.default_session = {
      user = "kiosk";
      command = ''
        ${pkgs.bash}/bin/bash -c '
          set -a
          [ -f ${envFile} ] && source ${envFile}
          set +a
          exec ${pkgs.cage}/bin/cage -s -- ${pkgs.foot}/bin/foot --config=${footConfig} -e ${workDashClient}/bin/work-dash-client
        '
      '';
    };
  };

  environment.systemPackages = [ pkgs.cage pkgs.foot workDashClient ];
  fonts.packages = [ pkgs.jetbrains-mono ];

  ###########################################################################
  # Networking — placeholder, fill in for the real LAN
  ###########################################################################

  networking.hostName = "work-dash-pi";
  # TODO: set a static LAN IP (or a DHCP reservation on the router) so the
  # laptop pusher and the server's Caddy/firewall rules have a stable target.
  # networking.interfaces.end0.ipv4.addresses = [{
  #   address = "192.168.1.50"; prefixLength = 24;
  # }];
  # networking.defaultGateway = "192.168.1.1";
  # networking.nameservers = [ "192.168.1.1" ];

  system.stateVersion = "25.11";
}
