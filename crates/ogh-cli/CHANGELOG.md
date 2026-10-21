# Changelog

## 0.1.0 (2026-10-21)


### ⚠ BREAKING CHANGES

* **cli:** ogh no longer opens the device itself; run ogh-daemon.

### Features

* A50 X protocol stack and ogh CLI ([030c8e3](https://github.com/Garulf/open-g-hub/commit/030c8e3cd388acd83377ee920eefcda41d8e97a7))
* **cli:** add ogh features, status, watch and sidetone ([f25964c](https://github.com/Garulf/open-g-hub/commit/f25964c2fb56379ac4ef885ad1e55d669ec5c997))
* **cli:** add Windows autostart command ([1d2b9a7](https://github.com/Garulf/open-g-hub/commit/1d2b9a71d6e82a5e1d7efca5534e5927cb9b2986))
* **cli:** talk to ogh-daemon over IPC ([da7ca73](https://github.com/Garulf/open-g-hub/commit/da7ca73754646aa121ebdf47782c71f7915ad3dc))
* **device:** add checked calls, typed reads and EQ and mixer readers ([30e75ed](https://github.com/Garulf/open-g-hub/commit/30e75edaf3d23d4db5ead8d4df413a6f677e6070))
* **gui:** add the ogh-gui window and tray, plus daemon and focus fixes ([f95052f](https://github.com/Garulf/open-g-hub/commit/f95052f4464dfc42381092531abaa3bfddf9fd9c))
* ogh-daemon service, profiles, focus and audio switching ([b565b7f](https://github.com/Garulf/open-g-hub/commit/b565b7f61d2959d3340589c8a3bb2f8dc6278598))
* **packaging:** ship the window and tray on Windows and NixOS ([77b98d8](https://github.com/Garulf/open-g-hub/commit/77b98d895b5991388197cc8e3edb200f93a1d59b))
* **protocol:** eq, mixer and settings writes seen in session B ([863b748](https://github.com/Garulf/open-g-hub/commit/863b748def32e65a9c5e68287056dfc14e40c272))


### Bug Fixes

* **cli:** keep status honest when the headset leaves and scope the permission hint ([87760e4](https://github.com/Garulf/open-g-hub/commit/87760e4c7e4ee282f53644ec6d5c3be60836c9f9))
* **device:** detect a refused hidraw open by asking the system about the path ([27ae842](https://github.com/Garulf/open-g-hub/commit/27ae842175d8d9dd7d5d4fa760b5f8a024411cc2))
* **device:** report parameter, index and sw_id refusals as Refused instead of an allowlist miss ([54783bf](https://github.com/Garulf/open-g-hub/commit/54783bf7a8b8325a7f1e03d82761271ea79ac99a))
* **packaging:** stop albedo deploy on a missing scratch dir and ship a Windows readme ([1bb23fb](https://github.com/Garulf/open-g-hub/commit/1bb23fb398d64af274c54c44abc045d54987ed94))
