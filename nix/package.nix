{
  lib,
  rustPlatform,
  pkg-config,
  udev,
}:

rustPlatform.buildRustPackage {
  pname = "open-g-hub";
  version = (lib.importTOML ../crates/ogh-daemon/Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
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
  '';

  meta = {
    description = "Open-source replacement for Logitech G Hub, for the Astro A50 X headset";
    homepage = "https://github.com/Garulf/open-g-hub";
    mainProgram = "ogh";
    platforms = lib.platforms.linux;
  };
}
