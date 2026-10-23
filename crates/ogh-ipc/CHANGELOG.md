# Changelog

## [0.1.1](https://github.com/Garulf/open-g-hub/compare/open-g-hub-v0.1.0...ogh-ipc-v0.1.1) (2026-10-23)


### Miscellaneous Chores

* **ogh-ipc:** Synchronize open-g-hub versions

## 0.1.0 (2026-10-21)


### Features

* **gui:** add the ogh-gui window and tray, plus daemon and focus fixes ([f95052f](https://github.com/Garulf/open-g-hub/commit/f95052f4464dfc42381092531abaa3bfddf9fd9c))
* **ipc:** add IPC message types and line codec ([434c5f4](https://github.com/Garulf/open-g-hub/commit/434c5f4a3414ab765d228bc6fd137af42f14b904))
* **ipc:** add per-user local socket transport ([fac5be1](https://github.com/Garulf/open-g-hub/commit/fac5be1d93b124b689bb956f51731df2f83cae17))
* ogh-daemon service, profiles, focus and audio switching ([b565b7f](https://github.com/Garulf/open-g-hub/commit/b565b7f61d2959d3340589c8a3bb2f8dc6278598))


### Bug Fixes

* **daemon:** close a dropped subscriber's connection ([29e692c](https://github.com/Garulf/open-g-hub/commit/29e692cf59644c0a3e6faca81ba08d3c1bc22bc2))
* **ipc:** lock the daemon socket against concurrent starts ([e344752](https://github.com/Garulf/open-g-hub/commit/e344752e89b163f1bf3a4869e2550732a0cec2f8))
