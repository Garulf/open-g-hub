{
  lib,
  rustPlatform,
  pkg-config,
  udev,
  libGL,
  libxkbcommon,
  wayland,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
}:

rustPlatform.buildRustPackage {
  pname = "open-g-hub";
  version = (lib.importTOML ../crates/ogh-daemon/Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../assets
      ../crates
      ../packaging
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ udev ];

  cargoBuildFlags = [
    "-p"
    "ogh-cli"
    "-p"
    "ogh-daemon"
    "-p"
    "ogh-gui"
  ];

  # CI runs the test suite; the build sandbox has no XDG_RUNTIME_DIR for the IPC tests.
  doCheck = false;

  postInstall = ''
    install -Dm644 packaging/linux/70-open-g-hub.rules \
      $out/lib/udev/rules.d/70-open-g-hub.rules
    install -Dm644 packaging/linux/ogh-daemon.service \
      $out/lib/systemd/user/ogh-daemon.service
    substituteInPlace $out/lib/systemd/user/ogh-daemon.service \
      --replace-fail "ExecStart=ogh-daemon" "ExecStart=$out/bin/ogh-daemon"
    install -Dm644 packaging/linux/ogh-tray.service \
      $out/lib/systemd/user/ogh-tray.service
    substituteInPlace $out/lib/systemd/user/ogh-tray.service \
      --replace-fail "ExecStart=ogh-tray" "ExecStart=$out/bin/ogh-tray"

    install -Dm644 packaging/linux/open-g-hub.desktop \
      $out/share/applications/open-g-hub.desktop
    substituteInPlace $out/share/applications/open-g-hub.desktop \
      --replace-fail "Exec=ogh-gui" "Exec=$out/bin/ogh-gui"
    for size in 32 64 256; do
      install -Dm644 assets/icon-$size.png \
        $out/share/icons/hicolor/''${size}x$size/apps/open-g-hub.png
    done
    install -Dm644 assets/icon.svg \
      $out/share/icons/hicolor/scalable/apps/open-g-hub.svg
  '';

  # eframe loads these at run time with dlopen, so they are not in the link line.
  postFixup = ''
    patchelf --add-rpath "${
      lib.makeLibraryPath [
        libGL
        libxkbcommon
        wayland
        libx11
        libxcursor
        libxi
        libxrandr
      ]
    }" $out/bin/ogh-gui
  '';

  meta = {
    description = "Open-source replacement for Logitech G Hub, for the Astro A50 X headset";
    homepage = "https://github.com/Garulf/open-g-hub";
    mainProgram = "ogh";
    platforms = lib.platforms.linux;
  };
}
