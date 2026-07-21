let
  # Sole recipient: the key on the machine used to build the ISO. Secrets
  # are decrypted to plaintext once at build time (see nixos/README.md) and
  # baked into the image — nothing decrypts at runtime on the Pi, so no
  # separate device identity is needed, and no private key ever touches git.
  cgppWslBox = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHflEu2znFC9TVaJ4dfVGzNZF0k/qmFWgJMYaIVCBe3r cgpp@wsl-box";

  admins = [ cgppWslBox ];
in
{
  "work-dash-pi.env.age".publicKeys = admins;
  "wifi.env.age".publicKeys = admins;
}
