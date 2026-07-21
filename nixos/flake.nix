{
  description = "work-dash Pi 4B kiosk: 7in DSI touchscreen, cage+foot, work-dash-client";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    nixos-raspberrypi.url = "github:nvmd/nixos-raspberrypi/main";
  };

  nixConfig = {
    extra-substituters = [ "https://nixos-raspberrypi.cachix.org" ];
    extra-trusted-public-keys = [ "nixos-raspberrypi.cachix.org-1:4iMO9LXa8BqhU+Rpg6LQKiGa2lsNh/j2oiYLNOQ5sPI=" ];
  };

  outputs = { self, nixpkgs, nixos-raspberrypi, ... }@inputs:
    let
      system = "aarch64-linux";
    in
    {
      # nixos-raspberrypi.lib.nixosSystem is a drop-in replacement for
      # nixpkgs.lib.nixosSystem that wires in the Pi-specific overlays/kernel.
      # (Confirmed from the flake's README; nixosSystemFull is the same but
      # applies RPi overlays globally — more rebuilds, not needed here.)
      nixosConfigurations.dashboard = nixos-raspberrypi.lib.nixosSystem {
        specialArgs = inputs;
        modules = [
          # Confirmed via `nix eval github:nvmd/nixos-raspberrypi#nixosModules
          # --apply builtins.attrNames`: nested attrset (`nixosModules.
          # raspberry-pi-4.{base,bluetooth,case-argonone,display-vc4}`), not
          # flat dotted-string keys. Pi 4B has no RP1 chip (that's Pi 5-only,
          # for RP1-connected DPI/composite/MIPI-DSI) so there's no
          # `display-rp1` module here — `display-vc4` only adds an Xorg
          # `PrimaryGPU` OutputClass, irrelevant under cage/Wayland (no X
          # server), so it's not imported either. DSI panel init on Pi 4 is
          # a config.txt dt-overlay instead — see the TODO in
          # configuration.nix.
          nixos-raspberrypi.nixosModules.raspberry-pi-4.base
          # `raspberry-pi-4.base` alone doesn't wire up an sd-card image
          # builder (that's opt-in, only added by nixos-raspberrypi's own
          # *-installer configs) — this is that same module, giving us
          # `config.system.build.sdImage` (exposed below as `packages.sdImage`)
          # so `nix build .#packages.aarch64-linux.sdImage` produces a
          # ready-to-flash, ready-to-boot image (not an "installer" that
          # still needs interactive wifi/first-boot setup — everything's
          # already baked in via configuration.nix).
          "${nixos-raspberrypi}/modules/installer/sd-card/sd-image-raspberrypi.nix"
          ./configuration.nix
        ];
      };

      packages.${system}.sdImage = self.nixosConfigurations.dashboard.config.system.build.sdImage;

      formatter.${system} = nixpkgs.legacyPackages.${system}.nixpkgs-fmt;
    };
}
