# Changelog

## [0.1.1](https://github.com/Garulf/open-g-hub/compare/open-g-hub-v0.1.0...open-g-hub-v0.1.1) (2026-10-23)


### Bug Fixes

* **audio:** default Linux endpoint names to the A50 X PipeWire nodes ([5ae48cd](https://github.com/Garulf/open-g-hub/commit/5ae48cd591fa7123873d053315933e95ff1e50b1))
* **audio:** default Linux endpoint names to the A50 X PipeWire nodes ([f2e798a](https://github.com/Garulf/open-g-hub/commit/f2e798a3f0fccbbe71fd555513b1c1a844b0f6cf))
* **audio:** scope the PipeWire test import to non-Windows builds ([7420243](https://github.com/Garulf/open-g-hub/commit/74202434832eb1ff4c7ecb93b246107418bbe6c7))

## 0.1.0 (2026-10-21)


### ⚠ BREAKING CHANGES

* **cli:** ogh no longer opens the device itself; run ogh-daemon.

### Features

* A50 X protocol stack and ogh CLI ([030c8e3](https://github.com/Garulf/open-g-hub/commit/030c8e3cd388acd83377ee920eefcda41d8e97a7))
* add albedo capture and deploy scripts ([e0b7f55](https://github.com/Garulf/open-g-hub/commit/e0b7f555da1fd6f4dfc943d5bd026ec49be4bad5))
* **audio:** switch default audio devices on undock and restore on dock ([e5b288f](https://github.com/Garulf/open-g-hub/commit/e5b288fc000ebb875c5e2193b836344fd2489330))
* **capture:** convert usbpcap captures to jsonl ([0425104](https://github.com/Garulf/open-g-hub/commit/04251042e31aae6e6b777bc8ab22e8f66c43363d))
* **cli:** add ogh features, status, watch and sidetone ([f25964c](https://github.com/Garulf/open-g-hub/commit/f25964c2fb56379ac4ef885ad1e55d669ec5c997))
* **cli:** add Windows autostart command ([1d2b9a7](https://github.com/Garulf/open-g-hub/commit/1d2b9a71d6e82a5e1d7efca5534e5927cb9b2986))
* **cli:** talk to ogh-daemon over IPC ([da7ca73](https://github.com/Garulf/open-g-hub/commit/da7ca73754646aa121ebdf47782c71f7915ad3dc))
* **daemon:** add config and profile store ([81b1318](https://github.com/Garulf/open-g-hub/commit/81b1318f9ac3e58c7a6d22ff9519a7c58de4b370))
* **daemon:** add controller for settings, profiles and status ([1dcd3d0](https://github.com/Garulf/open-g-hub/commit/1dcd3d08ba4cc15112c07ff4beb17a7d362985b6))
* **daemon:** add device snapshot and settings diff ([304ea34](https://github.com/Garulf/open-g-hub/commit/304ea346fe578e785902f01618c31092ded71f87))
* **daemon:** add device worker with reconnect and G Hub guard ([8c0f712](https://github.com/Garulf/open-g-hub/commit/8c0f712e9d8454bf8f686ff8e3015ba3feb63b92))
* **daemon:** serve the IPC API from ogh-daemon ([0be913c](https://github.com/Garulf/open-g-hub/commit/0be913c67320708ae1838157f35e99d9a16200bd))
* **device:** add checked calls, typed reads and EQ and mixer readers ([30e75ed](https://github.com/Garulf/open-g-hub/commit/30e75edaf3d23d4db5ead8d4df413a6f677e6070))
* **device:** allow the session B setting writes ([7430e35](https://github.com/Garulf/open-g-hub/commit/7430e35d47bbe9f33f0707f013f018efdb63c48b))
* **device:** hid session with allowlist and event queue ([d7e6211](https://github.com/Garulf/open-g-hub/commit/d7e62114b80982fe644db87d3e30cf47d7314957))
* **device:** parameter rules for the session B writes ([8282f4b](https://github.com/Garulf/open-g-hub/commit/8282f4b76587bfc15780e3c6838462b7d937d445))
* **focus:** switch profiles by focused app on Windows and Hyprland ([85cd450](https://github.com/Garulf/open-g-hub/commit/85cd450b41d31ae58d4d13c148db313ce73ba59d))
* **gui:** add status text and the tray menu model ([a1da31c](https://github.com/Garulf/open-g-hub/commit/a1da31c0d423735845b3ecda1d1820bcf1a655e8))
* **gui:** add the daemon link with reconnect and write coalescing ([ffae320](https://github.com/Garulf/open-g-hub/commit/ffae32081663e39eb68832d399ead94dc357047c))
* **gui:** add the equalizer and microphone pages ([e7490ca](https://github.com/Garulf/open-g-hub/commit/e7490ca174cdbea9cd756f4a76d376e4cf0a15a5))
* **gui:** add the Linux tray and the window launcher ([9e8fab5](https://github.com/Garulf/open-g-hub/commit/9e8fab5a950f68da75071ac914dd055a81389656))
* **gui:** add the ogh-gui window and tray, plus daemon and focus fixes ([f95052f](https://github.com/Garulf/open-g-hub/commit/f95052f4464dfc42381092531abaa3bfddf9fd9c))
* **gui:** add the profiles page ([362ff50](https://github.com/Garulf/open-g-hub/commit/362ff5019c3a8b6cdce8b443c556ea356043080f))
* **gui:** add the window with mixer and device pages ([d522f64](https://github.com/Garulf/open-g-hub/commit/d522f64d81dcb765659484835e91a861f583aff0))
* **gui:** add the Windows tray ([298412a](https://github.com/Garulf/open-g-hub/commit/298412acf58a18034652e5bc8eed03b164ab2537))
* **ipc:** add IPC message types and line codec ([434c5f4](https://github.com/Garulf/open-g-hub/commit/434c5f4a3414ab765d228bc6fd137af42f14b904))
* **ipc:** add per-user local socket transport ([fac5be1](https://github.com/Garulf/open-g-hub/commit/fac5be1d93b124b689bb956f51731df2f83cae17))
* ogh-daemon service, profiles, focus and audio switching ([b565b7f](https://github.com/Garulf/open-g-hub/commit/b565b7f61d2959d3340589c8a3bb2f8dc6278598))
* **packaging:** add a per-user Windows installer ([0e03d55](https://github.com/Garulf/open-g-hub/commit/0e03d55e1f2017ef1100948f9e912c0c1703a4e5))
* **packaging:** add a per-user Windows installer ([eabb1b0](https://github.com/Garulf/open-g-hub/commit/eabb1b068b6c88e7ba4ac8d1ec6714a553d7a9ad))
* **packaging:** ship the window and tray on Windows and NixOS ([77b98d8](https://github.com/Garulf/open-g-hub/commit/77b98d895b5991388197cc8e3edb200f93a1d59b))
* **protocol:** a50x feature messages observed in captures ([8e07fc2](https://github.com/Garulf/open-g-hub/commit/8e07fc29c510e0c2731421302f828af69d2cde8d))
* **protocol:** centurion framing and feature discovery ([bd0f5a6](https://github.com/Garulf/open-g-hub/commit/bd0f5a6228b70deffde3f4d1d3dcb20beb5f1c84))
* **protocol:** eq, mixer and settings writes seen in session B ([863b748](https://github.com/Garulf/open-g-hub/commit/863b748def32e65a9c5e68287056dfc14e40c272))


### Bug Fixes

* **audio:** abort a switch when its state cannot be saved ([f22aca4](https://github.com/Garulf/open-g-hub/commit/f22aca461cd1045d8cdef4612e1b7c06d95c02ec))
* **ci:** drop ogh-device's self dev-dependency so release-please can run ([2653a64](https://github.com/Garulf/open-g-hub/commit/2653a64b6cc42b742dde5843f2fb3447f11dfe07))
* **ci:** drop ogh-device's self dev-dependency so release-please can run ([9211aa2](https://github.com/Garulf/open-g-hub/commit/9211aa27e58293293f8be877b2005379c9cbd6a6))
* **cli:** keep status honest when the headset leaves and scope the permission hint ([87760e4](https://github.com/Garulf/open-g-hub/commit/87760e4c7e4ee282f53644ec6d5c3be60836c9f9))
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
* **device:** bound drain, clamp request timeout and tolerate unknown availability at open ([d28da60](https://github.com/Garulf/open-g-hub/commit/d28da603ea75667bb80e627579744931232d8403))
* **device:** close raw device paths and tighten the request gate ([35b4e60](https://github.com/Garulf/open-g-hub/commit/35b4e60a3c286bd7e5e2b67516637177fce79c88))
* **device:** detect a refused hidraw open by asking the system about the path ([27ae842](https://github.com/Garulf/open-g-hub/commit/27ae842175d8d9dd7d5d4fa760b5f8a024411cc2))
* **device:** fail closed when the Windows process list cannot be read ([48630ee](https://github.com/Garulf/open-g-hub/commit/48630eeffb6404b930b6f145520141b05e0da29c))
* **device:** report parameter, index and sw_id refusals as Refused instead of an allowlist miss ([54783bf](https://github.com/Garulf/open-g-hub/commit/54783bf7a8b8325a7f1e03d82761271ea79ac99a))
* **device:** rules for sidetone, balance, pinned mixer inputs and eq card shape ([4655ceb](https://github.com/Garulf/open-g-hub/commit/4655ceb425dd6cf5d649bb4201c377d229f6e755))
* **device:** treat an interrupted read as no data and report permission errors apart ([3692bac](https://github.com/Garulf/open-g-hub/commit/3692bac9a91cf570f607622525328405a7b71f0b))
* **focus:** fall back to the newest Hyprland instance when the signature is stale ([aae3fcb](https://github.com/Garulf/open-g-hub/commit/aae3fcb27aa124b56044f7e27874857d7d104792))
* **focus:** report missing Hyprland and apply the focused window's profile at start ([6f49149](https://github.com/Garulf/open-g-hub/commit/6f4914963109b5c3b22b672b945d74e5343269ad))
* **focus:** report no focused app when the query fails after a Hyprland outage ([402d21d](https://github.com/Garulf/open-g-hub/commit/402d21ddcb2502e9bf0905e504a20051c32794d0))
* **focus:** wait for the Hyprland socket instead of giving up ([2f0dcf6](https://github.com/Garulf/open-g-hub/commit/2f0dcf64d7ec61df8f06c6e39f64056ead32278c))
* guard against orphaned usbpcap captures ([e249942](https://github.com/Garulf/open-g-hub/commit/e249942a1b53166fa23d7f908fc84fe6ab154965))
* **gui:** disarm profile delete on any click and share the label helper ([7bffcb0](https://github.com/Garulf/open-g-hub/commit/7bffcb0240505a1859b0570b522c7815caf17f18))
* **gui:** exit non-zero when the tray service stops ([c99edb1](https://github.com/Garulf/open-g-hub/commit/c99edb1822fcce647eaeaf198e15848263cb3583))
* **gui:** keep the error banner dismissable and clear stale slider holds ([26c4019](https://github.com/Garulf/open-g-hub/commit/26c401984e11469b6678659591eaa71746b5e2e2))
* **gui:** keep writes before a save and stop the link cleanly ([1016d6c](https://github.com/Garulf/open-g-hub/commit/1016d6ca6d9be731e08a58700cc90e0d83dbc54a))
* **gui:** rebuild the Windows tray menu after every click ([00c982a](https://github.com/Garulf/open-g-hub/commit/00c982ab08153494d6f5aa6d792b63d210d1cb1e))
* **gui:** say Connecting until the daemon answers and surface real errors ([df678dc](https://github.com/Garulf/open-g-hub/commit/df678dcfb06c6cce2aeb3c66e15146a5e3a0ca40))
* **gui:** share the tray test fixture and escape menu labels once ([9e00e97](https://github.com/Garulf/open-g-hub/commit/9e00e9766eff4d3400b9ba870d35323f9f7bae34))
* **gui:** stop the eq editor writing without input and salt it per target ([f509fdc](https://github.com/Garulf/open-g-hub/commit/f509fdccc9c0115a4f53c6fb75dac77225a1889f))
* **ipc:** lock the daemon socket against concurrent starts ([e344752](https://github.com/Garulf/open-g-hub/commit/e344752e89b163f1bf3a4869e2550732a0cec2f8))
* **packaging:** stop albedo deploy on a missing scratch dir and ship a Windows readme ([1bb23fb](https://github.com/Garulf/open-g-hub/commit/1bb23fb398d64af274c54c44abc045d54987ed94))
* **packaging:** use the top-level X library names and tidy the tray README ([7546000](https://github.com/Garulf/open-g-hub/commit/7546000cebfb7b62f76d884dc1a8f395afee5867))
* **protocol:** drop the serial number builder that the deny table forbids ([ff29b60](https://github.com/Garulf/open-g-hub/commit/ff29b60d31fbd3d726a15239f79d340f2f67f632))
* **protocol:** pin mic out and bluetooth and require uniform ascending eq cards ([55bf7f2](https://github.com/Garulf/open-g-hub/commit/55bf7f229c61a317dca139388e5003a04f9ff241))
