open-g-hub for the Astro A50 X

Four programs, all in this folder. Keep them together.

  ogh-daemon.exe  talks to the base station and keeps its state
  ogh.exe         the command line that talks to the daemon
  ogh-gui.exe     the settings window
  ogh-tray.exe    a tray icon with the common settings; "Open open-g-hub" in its
                  menu starts the window

The window and the tray talk to the daemon, so the daemon has to be running.

If you used the setup program, the "Start open-g-hub when I sign in" box set
autostart for you, and the programs are in
%LOCALAPPDATA%\Programs\open-g-hub. Uninstall from Apps & features; your
profiles and settings stay.

To start the daemon and the tray at every logon, run this in a terminal in that
folder:

    ogh autostart on

"ogh autostart off" removes both and "ogh autostart status" shows each one.

Then check the headset with:

    ogh status

Quit Logitech G Hub (the agent and the tray icon) first. While it is running,
the daemon stays off the device.

Profiles and settings are stored in %APPDATA%\open-g-hub\config\config.toml.

Project page: https://github.com/Garulf/open-g-hub
