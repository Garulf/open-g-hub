# Changelog

## 0.1.0 (2026-10-21)


### Features

* **audio:** switch default audio devices on undock and restore on dock ([e5b288f](https://github.com/Garulf/open-g-hub/commit/e5b288fc000ebb875c5e2193b836344fd2489330))
* **daemon:** add config and profile store ([81b1318](https://github.com/Garulf/open-g-hub/commit/81b1318f9ac3e58c7a6d22ff9519a7c58de4b370))
* **daemon:** add controller for settings, profiles and status ([1dcd3d0](https://github.com/Garulf/open-g-hub/commit/1dcd3d08ba4cc15112c07ff4beb17a7d362985b6))
* **daemon:** add device snapshot and settings diff ([304ea34](https://github.com/Garulf/open-g-hub/commit/304ea346fe578e785902f01618c31092ded71f87))
* **daemon:** add device worker with reconnect and G Hub guard ([8c0f712](https://github.com/Garulf/open-g-hub/commit/8c0f712e9d8454bf8f686ff8e3015ba3feb63b92))
* **daemon:** serve the IPC API from ogh-daemon ([0be913c](https://github.com/Garulf/open-g-hub/commit/0be913c67320708ae1838157f35e99d9a16200bd))
* **focus:** switch profiles by focused app on Windows and Hyprland ([85cd450](https://github.com/Garulf/open-g-hub/commit/85cd450b41d31ae58d4d13c148db313ce73ba59d))
* **gui:** add the ogh-gui window and tray, plus daemon and focus fixes ([f95052f](https://github.com/Garulf/open-g-hub/commit/f95052f4464dfc42381092531abaa3bfddf9fd9c))
* ogh-daemon service, profiles, focus and audio switching ([b565b7f](https://github.com/Garulf/open-g-hub/commit/b565b7f61d2959d3340589c8a3bb2f8dc6278598))


### Bug Fixes

* **audio:** abort a switch when its state cannot be saved ([f22aca4](https://github.com/Garulf/open-g-hub/commit/f22aca461cd1045d8cdef4612e1b7c06d95c02ec))
* **daemon:** close a dropped subscriber's connection ([29e692c](https://github.com/Garulf/open-g-hub/commit/29e692cf59644c0a3e6faca81ba08d3c1bc22bc2))
* **daemon:** close the subscription test gap and install the signal handler before binding ([00ef902](https://github.com/Garulf/open-g-hub/commit/00ef902d6b3f6942c31d963769f42af70bd467f9))
* **daemon:** keep profile applications in order with in-flight writes ([2cf9512](https://github.com/Garulf/open-g-hub/commit/2cf95129f315b1f5e7c814a613230c7343858528))
* **daemon:** log startup failures ([28f39c7](https://github.com/Garulf/open-g-hub/commit/28f39c7f7e154490fac42f204dea8689cc18feac))
* **daemon:** reapply a manual profile after in-flight automatic writes ([7c7e2b8](https://github.com/Garulf/open-g-hub/commit/7c7e2b845b66fbc9125a77baff34096b10873bf5))
* **daemon:** recover a fixed config.toml, bound subscriber queues and rotate the log ([8bccfdd](https://github.com/Garulf/open-g-hub/commit/8bccfdd5875c7b811f175d03aefc32b043a0d1e9))
* **daemon:** reload config.toml before each change ([fe26e1f](https://github.com/Garulf/open-g-hub/commit/fe26e1f7b9772f20edac6424ffad3a07019cb945))
* **daemon:** save after the reapply and keep the default capture on the default ([54ce6af](https://github.com/Garulf/open-g-hub/commit/54ce6af08b762d8388848ea4297e0618f77647fc))
* **daemon:** stop stale refresh values and reconnect spin in device worker ([5323604](https://github.com/Garulf/open-g-hub/commit/5323604a45b9eb6c78351773196e29f27d631617))
* **daemon:** warn when a subscriber's connection cannot be closed early ([97d3c95](https://github.com/Garulf/open-g-hub/commit/97d3c95803e8308e7459583760334a93556f8460))
* **focus:** report missing Hyprland and apply the focused window's profile at start ([6f49149](https://github.com/Garulf/open-g-hub/commit/6f4914963109b5c3b22b672b945d74e5343269ad))
