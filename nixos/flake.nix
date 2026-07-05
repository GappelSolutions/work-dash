{
  description = "work-dash Pi 5 kiosk: 7in DSI touchscreen, cage+foot, work-dash-client";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    nixos-raspberrypi.url = "github:nvmd/nixos-raspberrypi/main";
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
          # TODO(verify): confirm the exact attribute path/spelling of these
          # module outputs against nixos-raspberrypi's own flake.nix — the
          # README documents the *names* ("raspberry-pi-5.display-rp1" for
          # RP1-connected DPI/composite/MIPI-DSI displays) but not whether
          # they're nested attrsets or flat quoted keys under `nixosModules`.
          # Run `nix flake show github:nvmd/nixos-raspberrypi` to check the
          # real output tree before first build.
          nixos-raspberrypi.nixosModules."raspberry-pi-5.base"
          nixos-raspberrypi.nixosModules."raspberry-pi-5.display-rp1"
          ./configuration.nix
        ];
      };

      formatter.${system} = nixpkgs.legacyPackages.${system}.nixpkgs-fmt;
    };
}
