# Changelog

## [0.9.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.8.0...v0.9.0) (2026-07-24)


### ⚠ BREAKING CHANGES

* **experiments:** ExperimentCreate requires runtime_group_id, arms of {runtime_id, arm_label}, and a typed goal_json; Arm.weight and the untyped goal_json map are removed; ExperimentStatus::Concluded is now Ended.

### Features

* add environment_ref to Runtime schema ([#45](https://github.com/introspection-org/introspection-rust-sdk/issues/45)) ([d7a9937](https://github.com/introspection-org/introspection-rust-sdk/commit/d7a9937ebf6d5b8fbbcc794afc8696e6f8e85519))


### Bug Fixes

* **experiments:** align the experiments contract with the CP API ([#47](https://github.com/introspection-org/introspection-rust-sdk/issues/47)) ([15c7760](https://github.com/introspection-org/introspection-rust-sdk/commit/15c7760adbd81d8ba9f94b812b4811c8d7b414d6))

## [0.8.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.7.0...v0.8.0) (2026-07-18)


### ⚠ BREAKING CHANGES

* EventListParams requires event_name (exactly one family per request) and no longer implements Default; the untyped Event record (event_id/name/extra) is replaced by the discriminated Event union with typed payloads; grain and pattern_id params are removed.

### Features

* **runner:** add DP telemetry reads (conversations/events/metrics) with optional Arrow ([#37](https://github.com/introspection-org/introspection-rust-sdk/issues/37)) ([339f23a](https://github.com/introspection-org/introspection-rust-sdk/commit/339f23a0e070e92f9de29772d978ff70a484c93a))
* transparent gzip response decoding (reqwest gzip feature) ([#40](https://github.com/introspection-org/introspection-rust-sdk/issues/40)) ([76fe159](https://github.com/introspection-org/introspection-rust-sdk/commit/76fe159890ae7ea5dfe5c2a9c47af7b01b5df16d))
* typed discriminated events read (required event_name) ([#39](https://github.com/introspection-org/introspection-rust-sdk/issues/39)) ([4ae14e8](https://github.com/introspection-org/introspection-rust-sdk/commit/4ae14e8cb1338d2439984c806ad83c37cef98e4a))


### Bug Fixes

* align SDK execution contracts ([#42](https://github.com/introspection-org/introspection-rust-sdk/issues/42)) ([2d5dad9](https://github.com/introspection-org/introspection-rust-sdk/commit/2d5dad93e7e06ca0bf23cb6d1b19a6518286b684))
* keep runtime SDK surface read and run only ([#44](https://github.com/introspection-org/introspection-rust-sdk/issues/44)) ([137ee89](https://github.com/introspection-org/introspection-rust-sdk/commit/137ee8970bc436e37bda2b653bc76b1809f7b102))
* make cancel options canonical ([#43](https://github.com/introspection-org/introspection-rust-sdk/issues/43)) ([1c6ba7d](https://github.com/introspection-org/introspection-rust-sdk/commit/1c6ba7dfa78f202da7f12971d0081a3da9cb654d))

## [0.7.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.6.5...v0.7.0) (2026-07-05)


### Features

* **ci:** adopt release-please for versioning; rename VERSION to version.txt ([#32](https://github.com/introspection-org/introspection-rust-sdk/issues/32)) ([4aa6468](https://github.com/introspection-org/introspection-rust-sdk/commit/4aa646841cc8817bf8cb059027fad264fd13e22b))
* **ci:** release-please cuts the tag on release-PR merge ([#34](https://github.com/introspection-org/introspection-rust-sdk/issues/34)) ([25c9972](https://github.com/introspection-org/introspection-rust-sdk/commit/25c99729cb3d852fb7c3e19b6328f4c6ca0f2151))
