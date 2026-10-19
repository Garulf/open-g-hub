self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.open-g-hub;
in
{
  options.programs.open-g-hub = {
    enable = lib.mkEnableOption "open-g-hub, a replacement for Logitech G Hub for the Astro A50 X";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "open-g-hub.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The open-g-hub package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    services.udev.packages = [ cfg.package ];
    environment.systemPackages = [ cfg.package ];

    systemd.user.services.ogh-daemon = {
      description = "open-g-hub daemon for the Astro A50 X";
      after = [
        "graphical-session.target"
        "pipewire.service"
      ];
      partOf = [ "graphical-session.target" ];
      wantedBy = [ "graphical-session.target" ];
      path = [
        pkgs.wireplumber
        pkgs.pipewire
      ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/ogh-daemon";
        Restart = "on-failure";
        RestartSec = 2;
      };
    };
  };
}
