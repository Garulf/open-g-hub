# Changelog

## 0.1.0 (2026-10-21)


### Features

* **focus:** switch profiles by focused app on Windows and Hyprland ([85cd450](https://github.com/Garulf/open-g-hub/commit/85cd450b41d31ae58d4d13c148db313ce73ba59d))
* **gui:** add the ogh-gui window and tray, plus daemon and focus fixes ([f95052f](https://github.com/Garulf/open-g-hub/commit/f95052f4464dfc42381092531abaa3bfddf9fd9c))
* ogh-daemon service, profiles, focus and audio switching ([b565b7f](https://github.com/Garulf/open-g-hub/commit/b565b7f61d2959d3340589c8a3bb2f8dc6278598))


### Bug Fixes

* **ci:** drop ogh-device's self dev-dependency so release-please can run ([2653a64](https://github.com/Garulf/open-g-hub/commit/2653a64b6cc42b742dde5843f2fb3447f11dfe07))
* **focus:** fall back to the newest Hyprland instance when the signature is stale ([aae3fcb](https://github.com/Garulf/open-g-hub/commit/aae3fcb27aa124b56044f7e27874857d7d104792))
* **focus:** report missing Hyprland and apply the focused window's profile at start ([6f49149](https://github.com/Garulf/open-g-hub/commit/6f4914963109b5c3b22b672b945d74e5343269ad))
* **focus:** report no focused app when the query fails after a Hyprland outage ([402d21d](https://github.com/Garulf/open-g-hub/commit/402d21ddcb2502e9bf0905e504a20051c32794d0))
* **focus:** wait for the Hyprland socket instead of giving up ([2f0dcf6](https://github.com/Garulf/open-g-hub/commit/2f0dcf64d7ec61df8f06c6e39f64056ead32278c))
