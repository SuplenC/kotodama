# Changelog

## 1.0.0 (2026-10-19)


### ⚠ BREAKING CHANGES

* **kotodamad:** Learned choices take over after 2 picks instead of 3, and history.json records which Results a choice was picked over.

### Features

* a stable Identity for the same thing from several Extensions ([3ecf012](https://github.com/SuplenC/kotodama/commit/3ecf012ec0569519ea4acd5e288af7e3971e8ea5))
* **apps:** the apps Bundled extension ([5678232](https://github.com/SuplenC/kotodama/commit/5678232dc578f20ce147b32e1613262c7a4240f1))
* **apps:** the apps Bundled extension ([388ce3f](https://github.com/SuplenC/kotodama/commit/388ce3f5c4a52aec75ea181c8069239398ea9947))
* **clipboard:** the clipboard history Bundled extension ([e4b7ee0](https://github.com/SuplenC/kotodama/commit/e4b7ee0f8a7ed997a2e65121972a2391f6eb6a40))
* **clipboard:** the clipboard history Bundled extension ([3729858](https://github.com/SuplenC/kotodama/commit/37298580e728e7b5cb01c7e07a696e54958f82e7))
* **daemon:** parse inline Arguments, ask for missing ones, and notify when a job ends ([5ca844b](https://github.com/SuplenC/kotodama/commit/5ca844b3705ee8ef22e0a9de0cac7ee6c63a0f56))
* **examples:** add a Timer to the shell example ([9df3fda](https://github.com/SuplenC/kotodama/commit/9df3fda7b1ee01fc440f6f4632d7e4a3b8308747))
* give Items an identity that several Extensions share ([f9935f6](https://github.com/SuplenC/kotodama/commit/f9935f69660939632c3aff2954a51149ad2c3e50))
* **kotodama-qml:** the Quickshell Front end and the kotodama command ([25c87ec](https://github.com/SuplenC/kotodama/commit/25c87ecf047535ab91a86f1c150fb496642f0b9d))
* **kotodama-qml:** the Quickshell Front end and the kotodama command ([862eac1](https://github.com/SuplenC/kotodama/commit/862eac1465f9367973931dcf8b322bcbe28fc821)), closes [#5](https://github.com/SuplenC/kotodama/issues/5)
* **kotodamad:** Frecency, Learned choices, Aliases and Quicklinks ([#20](https://github.com/SuplenC/kotodama/issues/20)) ([3838c28](https://github.com/SuplenC/kotodama/commit/3838c284daef42b165772dfcab797751892bf596))
* **kotodamad:** history settings in the config, and an opt-in usage log ([c192c4a](https://github.com/SuplenC/kotodama/commit/c192c4aea57e7bc0ac31406e203b39f467e9f98b))
* **kotodamad:** history settings in the config, and an opt-in usage log ([4b822cd](https://github.com/SuplenC/kotodama/commit/4b822cd8f07077cbd8994e19b40fc134517cc4a2))
* **kotodamad:** rank by Tier first, and learn in both directions ([d910178](https://github.com/SuplenC/kotodama/commit/d91017880ff4a3a52ec754fea1bbeec368938483))
* **kotodamad:** the Daemon: index, nucleo ranking, Extension host ([#19](https://github.com/SuplenC/kotodama/issues/19)) ([2d9bef3](https://github.com/SuplenC/kotodama/commit/2d9bef33098edb13d35e1e6d5af235dde4b078b7))
* **kotodamad:** the empty query, and Frecency by time of day ([2e80c14](https://github.com/SuplenC/kotodama/commit/2e80c1463b1547ec5b4f563d3d44e7f101aa5ffb))
* **kotodamad:** the empty query, and Frecency by time of day ([d6525b4](https://github.com/SuplenC/kotodama/commit/d6525b4d37dd02f257e8eeec2f4034e159a93f23))
* package Kotodama with Nix, start the Daemon by socket activation ([75f7e1f](https://github.com/SuplenC/kotodama/commit/75f7e1fa0427a92887dad6336758eca069a67fd3))
* package Kotodama with Nix, start the Daemon by socket activation ([378bcb3](https://github.com/SuplenC/kotodama/commit/378bcb3f3fab0547c521c836a07440dc658cb706))
* **protocol:** add the kotodama-protocol crate and the shell example Extension ([#18](https://github.com/SuplenC/kotodama/issues/18)) ([48061e0](https://github.com/SuplenC/kotodama/commit/48061e094d9367be8dbca397d1fadbb650d68966))
* **qml:** add the View stack, the Action panel, and running jobs to the menu ([217d1c6](https://github.com/SuplenC/kotodama/commit/217d1c62b27cc99ffb0c6d587b8ab9d03b4f4136))
* Views, the Action panel, Arguments, and job progress ([7b6db93](https://github.com/SuplenC/kotodama/commit/7b6db93b2eb99086cf0b95f6a0022ff0ef760bc5))


### Bug Fixes

* **apps:** address review: identity, leaner diffing, rescans off the main thread ([59b4fd7](https://github.com/SuplenC/kotodama/commit/59b4fd7edfdd3e2392129c8babf2b074ebf21390))
* bump the lockfile on release, and check apps in the NixOS system profile ([81d0251](https://github.com/SuplenC/kotodama/commit/81d02515bcd388be32dcb82bf5a69240da3288ec))
* **clipboard:** address review findings ([44c98d4](https://github.com/SuplenC/kotodama/commit/44c98d4eaf5642c7c32d18efc6589bf916be3223))
* **daemon:** fail the running jobs of an Extension that stops ([32240ac](https://github.com/SuplenC/kotodama/commit/32240acb3265c125331b919192dcddeb3fd67726))
* **daemon:** never block running an Action on an Extension that stopped reading ([c7bb1e0](https://github.com/SuplenC/kotodama/commit/c7bb1e014d9a031ad081ad07d8186384f943a245))
* **daemon:** never block running an Action on an Extension that stopped reading ([b505a21](https://github.com/SuplenC/kotodama/commit/b505a21b7888ba74181508da468447c0c6c46024))
* **examples:** send the timer's messages in a fixed order ([1a5cef2](https://github.com/SuplenC/kotodama/commit/1a5cef279768ca9c9cf9b20c59866836c88fa7ad))
* **kotodamad:** finer time-of-day shares, a clock that counts suspend ([0810b1c](https://github.com/SuplenC/kotodama/commit/0810b1c8880adbb4b671401e2038fbbc7ea6e534))
* **kotodamad:** lead with the Result picked through, whatever it matched ([66018e6](https://github.com/SuplenC/kotodama/commit/66018e65dd2652a5f6bd77d7915af51a0a818aaf))
* **kotodamad:** rotate a full usage log at once, and bound what waits for it ([338a00a](https://github.com/SuplenC/kotodama/commit/338a00ac66c8bd21748afe0ee863875963de91b5))
* **qml:** keep Views open when the menu closes ([78c67e2](https://github.com/SuplenC/kotodama/commit/78c67e291ee70145733f3d9d85e3840da2efc0e3))


### Performance Improvements

* **qml:** build a Result's Action panel only when it is asked for ([6f80fd6](https://github.com/SuplenC/kotodama/commit/6f80fd6e25ed97a22396e7c4703cf7dcffda0039))
