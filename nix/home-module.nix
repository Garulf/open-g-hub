self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.open-g-hub;
in
{
  options.services.open-g-hub = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to run the open-g-hub daemon for the Astro A50 X as a user service.
        This module does not install the udev rule that lets your user open the
        headset. Use the NixOS module for that, or add the package to
        `services.udev.packages` in your system configuration.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "open-g-hub.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The open-g-hub package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.ogh-daemon = {
      Unit = {
        Description = "open-g-hub daemon for the Astro A50 X";
        After = [
          "graphical-session.target"
          "pipewire.service"
        ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/ogh-daemon";
        Restart = "on-failure";
        RestartSec = 2;
        Environment = "PATH=${
          lib.makeBinPath [
            pkgs.wireplumber
            pkgs.pipewire
          ]
        }";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
