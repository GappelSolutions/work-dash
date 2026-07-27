let
  # Builder key: whoever holds this can re-encrypt/edit secrets from a dev
  # machine (`agenix -e`). Not present on the Pi itself.
  cgppWslBox = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHflEu2znFC9TVaJ4dfVGzNZF0k/qmFWgJMYaIVCBe3r cgpp@wsl-box";

  # Device key: baked into the image once at flash time
  # (~/.work-dash-pi-secrets/device.key, git-ignored, generated via
  # `age-keygen`) so agenix can decrypt these files at *activation* time on
  # the Pi itself — this is what lets `system.autoUpgrade` pull new secrets
  # from git and apply them with no SSH/KVM access to the device.
  piDevice = "age1q8zh82s3mma4hy7egfq2xzzqxwjr4s98qmxct6jpqe8spnr6evpsvmecez";

  admins = [ cgppWslBox piDevice ];
in
{
  "work-dash-pi.env.age".publicKeys = admins;
  "wifi.env.age".publicKeys = admins;
}
