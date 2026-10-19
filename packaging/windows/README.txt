open-g-hub for the Astro A50 X

ogh-daemon.exe talks to the base station and keeps its state. ogh.exe is the
command line that talks to the daemon. Keep both files in the same folder.

To start the daemon at every logon, run this in a terminal in that folder:

    ogh autostart on

Then check the headset with:

    ogh status

Quit Logitech G Hub (the agent and the tray icon) first. While it is running,
the daemon stays off the device.

Profiles and settings are stored in %APPDATA%\open-g-hub\config\config.toml.

Project page: https://github.com/Garulf/open-g-hub
