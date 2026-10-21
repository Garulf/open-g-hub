# Changelog

## 0.1.0 (2026-10-21)


### Features

* A50 X protocol stack and ogh CLI ([030c8e3](https://github.com/Garulf/open-g-hub/commit/030c8e3cd388acd83377ee920eefcda41d8e97a7))
* **device:** add checked calls, typed reads and EQ and mixer readers ([30e75ed](https://github.com/Garulf/open-g-hub/commit/30e75edaf3d23d4db5ead8d4df413a6f677e6070))
* **device:** allow the session B setting writes ([7430e35](https://github.com/Garulf/open-g-hub/commit/7430e35d47bbe9f33f0707f013f018efdb63c48b))
* **device:** hid session with allowlist and event queue ([d7e6211](https://github.com/Garulf/open-g-hub/commit/d7e62114b80982fe644db87d3e30cf47d7314957))
* **device:** parameter rules for the session B writes ([8282f4b](https://github.com/Garulf/open-g-hub/commit/8282f4b76587bfc15780e3c6838462b7d937d445))
* ogh-daemon service, profiles, focus and audio switching ([b565b7f](https://github.com/Garulf/open-g-hub/commit/b565b7f61d2959d3340589c8a3bb2f8dc6278598))


### Bug Fixes

* **ci:** drop ogh-device's self dev-dependency so release-please can run ([2653a64](https://github.com/Garulf/open-g-hub/commit/2653a64b6cc42b742dde5843f2fb3447f11dfe07))
* **ci:** drop ogh-device's self dev-dependency so release-please can run ([9211aa2](https://github.com/Garulf/open-g-hub/commit/9211aa27e58293293f8be877b2005379c9cbd6a6))
* **device:** bound drain, clamp request timeout and tolerate unknown availability at open ([d28da60](https://github.com/Garulf/open-g-hub/commit/d28da603ea75667bb80e627579744931232d8403))
* **device:** close raw device paths and tighten the request gate ([35b4e60](https://github.com/Garulf/open-g-hub/commit/35b4e60a3c286bd7e5e2b67516637177fce79c88))
* **device:** detect a refused hidraw open by asking the system about the path ([27ae842](https://github.com/Garulf/open-g-hub/commit/27ae842175d8d9dd7d5d4fa760b5f8a024411cc2))
* **device:** fail closed when the Windows process list cannot be read ([48630ee](https://github.com/Garulf/open-g-hub/commit/48630eeffb6404b930b6f145520141b05e0da29c))
* **device:** report parameter, index and sw_id refusals as Refused instead of an allowlist miss ([54783bf](https://github.com/Garulf/open-g-hub/commit/54783bf7a8b8325a7f1e03d82761271ea79ac99a))
* **device:** rules for sidetone, balance, pinned mixer inputs and eq card shape ([4655ceb](https://github.com/Garulf/open-g-hub/commit/4655ceb425dd6cf5d649bb4201c377d229f6e755))
* **device:** treat an interrupted read as no data and report permission errors apart ([3692bac](https://github.com/Garulf/open-g-hub/commit/3692bac9a91cf570f607622525328405a7b71f0b))
* **protocol:** drop the serial number builder that the deny table forbids ([ff29b60](https://github.com/Garulf/open-g-hub/commit/ff29b60d31fbd3d726a15239f79d340f2f67f632))
