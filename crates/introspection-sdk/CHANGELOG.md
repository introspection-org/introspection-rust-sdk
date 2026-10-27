# Changelog

## [0.34.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.33.1...v0.34.0) (2026-10-27)


### ⚠ BREAKING CHANGES

* Annotations::create returns AnnotationCreated instead of (), AnnotationMutation has a new variant, ArrowPage has a new field, and RuntimeUpdateParams.yanked_reason is Option<Option<String>>. StreamOptions gains `max_frame_bytes` (one SSE frame is capped at 8 MiB by default) and IntrospectionAPIError gains `SseFrameTooLarge`, so a struct literal that names every StreamOptions field, or an exhaustive match on the error, needs the new member.
* `ConversationExportFormat` has two new variants and is now `#[non_exhaustive]`, so an exhaustive `match` on it needs a wildcard arm. `EventListParams` has new public fields, so a struct literal without `..EventListParams::new(family)` no longer compiles. `Runtime` gains the public fields `image_revision`, `image_commit_subject`, `image_repository`, `runtime_group_type` and `template_ref`, so a struct literal that names every field needs them.
* FileCreateText, FileUpload, AutomationMetadata, AnnotationEventOptions and ImageListParams gain public fields, so a struct literal that names every field needs the new one (or `..Default::default()`).
* Connectors::get/create/update/delete/authorize and Recipes::get take a new trailing `project` argument (pass `None` for the old behaviour). ConnectorListParams has a new public `project` field. VariableUpdateParams::environment and CredentialUpdateParams::environment are `Option<Option<Lane>>`; wrap an existing value in `Some(...)`. The span attribute types gain public fields for `introspection.ingest` and `introspection.history.{diverged,stored}`, so a struct literal that names every field needs the new ones.
* RunRequest sends environment, recipe_id and bindings_required; RuntimeHandle::with_project; Runner::session_token ([#170](https://github.com/introspection-org/introspection-rust-sdk/issues/170))
* TaskFileRef.id is a Uuid; a 410 on a run stream ends it with StreamHistoryUnavailable instead of StreamIncomplete, and a 429 that outlives the recovery window with RunNotReady instead of Timeout. New public fields on TaskRunCreate, TaskListParams and StreamOptions, and new IntrospectionAPIError variants.
* response types leave out `None` fields when they serialize, and their map-valued fields are `BTreeMap` instead of `HashMap`. `Experiment::created_by_member_id`, `Arm::id` and `Arm::initial_weight` are required. `Runtime::is_active` and `Runtime::allow_hot_swap` are removed: the server sends neither, so they were always `false`. `IntrospectionAttributes::run` and `task` are now `IntrospectionRun` and `IntrospectionTask` instead of `IdRef`. Open maps on the GenAI attribute and message types are named `other`.

### Features

* authoring writes for runtimes, recipes, endpoints and repositories behind an `authoring` feature ([#176](https://github.com/introspection-org/introspection-rust-sdk/issues/176)) ([bbc9e2b](https://github.com/introspection-org/introspection-rust-sdk/commit/bbc9e2b7910c4402f4b042a981615b6422276c28))
* file TTLs, JSON text file versions, free-form automation metadata, annotation comments with a snapshot, image list filters ([#174](https://github.com/introspection-org/introspection-rust-sdk/issues/174)) ([7fe2f03](https://github.com/introspection-org/introspection-rust-sdk/commit/7fe2f0396ff9a9acd1c65778cbf9404fd8e6d89d))
* project selectors on connectors and Recipes::get, null environment on setting updates, the ingest stamp and stored-history match on spans ([#171](https://github.com/introspection-org/introspection-rust-sdk/issues/171)) ([2197ebe](https://github.com/introspection-org/introspection-rust-sdk/commit/2197ebe5e399e4ca8fad1d873f9a07f43f41375a))
* raw metrics query, replay and ATIF exports, merged event families, and the runtime's image and group fields ([#175](https://github.com/introspection-org/introspection-rust-sdk/issues/175)) ([a0875db](https://github.com/introspection-org/introspection-rust-sdk/commit/a0875db8ef18a11ae28183799836c128ab1cbdac))
* response types serialize back to the server JSON, with a strict-schema feature ([#163](https://github.com/introspection-org/introspection-rust-sdk/issues/163)) ([554c901](https://github.com/introspection-org/introspection-rust-sdk/commit/554c9014fc7635ad33e7f5280b91826946362bdf))
* RunRequest sends environment, recipe_id and bindings_required; RuntimeHandle::with_project; Runner::session_token ([#170](https://github.com/introspection-org/introspection-rust-sdk/issues/170)) ([c0b6e36](https://github.com/introspection-org/introspection-rust-sdk/commit/c0b6e36bde84e758b089856afcde30953d1df0c3))
* SDK gaps from the CLI 0.34 conversion: Arrow schema, annotation reviewer ids and created event, null yank reason, SSE frame cap, null-tolerant reads ([#177](https://github.com/introspection-org/introspection-rust-sdk/issues/177)) ([bf833b2](https://github.com/introspection-org/introspection-rust-sdk/commit/bf833b23875bf8f0ae4b96d52f88f6ea050ff44b))
* task run delivery ids, runtime list filter, stream cursors and raw frames ([#172](https://github.com/introspection-org/introspection-rust-sdk/issues/172)) ([4ba844b](https://github.com/introspection-org/introspection-rust-sdk/commit/4ba844bfb136438d99de8354143b5b5dc05bd1c7))


### Bug Fixes

* decode a GenAI message that has no parts ([#161](https://github.com/introspection-org/introspection-rust-sdk/issues/161)) ([bec67f5](https://github.com/introspection-org/introspection-rust-sdk/commit/bec67f5a654268e9312d1dd2ecd831e4a3d0533e))
* serialize a message without parts back without parts ([#173](https://github.com/introspection-org/introspection-rust-sdk/issues/173)) ([acfcaa5](https://github.com/introspection-org/introspection-rust-sdk/commit/acfcaa5afb6db1f1f6c2b5d3d1efec4863e1e118))

## [0.33.1](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.33.0...v0.33.1) (2026-10-24)


### Bug Fixes

* **walk:** a poll reads a 404 as not yet ([#153](https://github.com/introspection-org/introspection-rust-sdk/issues/153)) ([9f1d83a](https://github.com/introspection-org/introspection-rust-sdk/commit/9f1d83ade8d2c8fb1fd2c519756d642e5352f014))
* **walk:** concurrent walks never sweep each other's project ([#154](https://github.com/introspection-org/introspection-rust-sdk/issues/154)) ([dda9e7d](https://github.com/introspection-org/introspection-rust-sdk/commit/dda9e7d391d84586ac37259f7d326556a78d0b00))

## [0.33.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.32.0...v0.33.0) (2026-10-23)


### ⚠ BREAKING CHANGES

* add the project administration and eval resources the CLI calls ([#123](https://github.com/introspection-org/introspection-rust-sdk/issues/123))

### Features

* add the project administration and eval resources the CLI calls ([#123](https://github.com/introspection-org/introspection-rust-sdk/issues/123)) ([82cce9d](https://github.com/introspection-org/introspection-rust-sdk/commit/82cce9d6d50cb0bfee3c6308acd2deb538f70cdb))

## [0.32.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.31.1...v0.32.0) (2026-10-23)


### ⚠ BREAKING CHANGES

* **conversations:** `ConversationContentClass` is replaced by `ConversationContains` (`InputUser`, `OutputText`, `InputToolResponse`, `OutputToolCall`, `AnyInputToolResponse`, `AnyOutputToolCall`, `Tool`, `Operation`), `IntrospectionContent` by `IntrospectionContains`, and the `content` field on the list params and on `IntrospectionAttributes` by `contains`.
* **aauth:** the aauth-person-server crate is no longer maintained.

### Features

* **conversations:** add the contains filter on items and deprecate operation_name ([#143](https://github.com/introspection-org/introspection-rust-sdk/issues/143)) ([d2a54ae](https://github.com/introspection-org/introspection-rust-sdk/commit/d2a54ae73d8d20cc9de898a3c471911611efe423))


### Bug Fixes

* **conversations:** read a conversation from a data plane that predates total_input_tokens ([#148](https://github.com/introspection-org/introspection-rust-sdk/issues/148)) ([cd5ba0b](https://github.com/introspection-org/introspection-rust-sdk/commit/cd5ba0bb790b04fe702a3656ce03f4d0355eb080))


### Code Refactoring

* **aauth:** retire the Rust Person Server and Access Server ([#145](https://github.com/introspection-org/introspection-rust-sdk/issues/145)) ([f8f8bf4](https://github.com/introspection-org/introspection-rust-sdk/commit/f8f8bf43c5c012c18356d8bfa8d7a3c2d4a4e3ea))

## [0.31.1](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.31.0...v0.31.1) (2026-10-21)


### Bug Fixes

* **examples:** show the helpers each docs region calls ([#140](https://github.com/introspection-org/introspection-rust-sdk/issues/140)) ([c51acb8](https://github.com/introspection-org/introspection-rust-sdk/commit/c51acb8b32d940b719f68467dfb049ddf4f4e9d7))

## [0.31.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.30.0...v0.31.0) (2026-10-21)


### Features

* **examples:** add compiled docs-region examples for patterns, observe and sharing pages ([#126](https://github.com/introspection-org/introspection-rust-sdk/issues/126)) ([8d14eaf](https://github.com/introspection-org/introspection-rust-sdk/commit/8d14eafdf9b2e4625b03c4d53aa9f435100c5df6))
* **examples:** add docs region examples for the patterns pages ([#128](https://github.com/introspection-org/introspection-rust-sdk/issues/128)) ([d2dd570](https://github.com/introspection-org/introspection-rust-sdk/commit/d2dd570bc3c4bb663ef79ba7c65e37ebddd8c078))
* **examples:** compiled docs-region examples for the Rust reference and authentication pages ([#127](https://github.com/introspection-org/introspection-rust-sdk/issues/127)) ([5e592c3](https://github.com/introspection-org/introspection-rust-sdk/commit/5e592c3ec1db8c1ce3569c42f45a36ead4ba60fd))

## [0.30.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.29.0...v0.30.0) (2026-10-20)


### Features

* add the aauth-protocol and aauth-person-server crates in a Cargo workspace ([#124](https://github.com/introspection-org/introspection-rust-sdk/issues/124)) ([fe9ce52](https://github.com/introspection-org/introspection-rust-sdk/commit/fe9ce5256e2ac6e39aa4b278a1a03fc2324757f6))
* **sdk:** client.aauth() approvals and missions, experimental ([#130](https://github.com/introspection-org/introspection-rust-sdk/issues/130)) ([bacf636](https://github.com/introspection-org/introspection-rust-sdk/commit/bacf63631272884091934c70946d4d4363ea7e83))

## [0.29.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.28.0...v0.29.0) (2026-10-15)


### ⚠ BREAKING CHANGES

* `MetricSpec::measure` is `Option<String>`, and `MetricsResponse::data` is `Vec<MetricRow>` and `meta` is `Option<MetricsMeta>` rather than JSON values. `ConversationListParams` and `ConnectionTokenParams` gain fields, so an exhaustive struct literal needs `..Default::default()`.

### Features

* endpoint bindings, typed metrics rows, and the trace and token selectors ([#120](https://github.com/introspection-org/introspection-rust-sdk/issues/120)) ([74337dd](https://github.com/introspection-org/introspection-rust-sdk/commit/74337dd08074b7ca14342191e389707b0285dc26))

## [0.28.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.27.0...v0.28.0) (2026-10-15)


### ⚠ BREAKING CHANGES

* **tags:** `client.project_labels()` is now `client.tags()`, and `ProjectLabel{,Create,Update,ListParams}` are replaced by `Tag`, `TagCreate`, `TagUpdate` and `TagListParams`, which take a `TagKind`.
* **repositories:** `RepositoryProvider::Hosted` is renamed

### Features

* **tags:** the tag catalogue, keyed by access or annotation ([#119](https://github.com/introspection-org/introspection-rust-sdk/issues/119)) ([14fafce](https://github.com/introspection-org/introspection-rust-sdk/commit/14fafcecd5d86e8042a6f114e08f420a4a13cf06))


### Bug Fixes

* **repositories:** the platform-served provider is `introspection` ([#117](https://github.com/introspection-org/introspection-rust-sdk/issues/117)) ([bb87b2a](https://github.com/introspection-org/introspection-rust-sdk/commit/bb87b2add1c9bd09420444b9348ee74afc2f254c))

## [0.27.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.26.0...v0.27.0) (2026-10-14)


### ⚠ BREAKING CHANGES

* **connectors:** `approval_policy` and `person_server_mode` are removed from `Connector` and `ConnectorCreateParams`. The control plane retired both; a required `approval_policy` made every current connector read fail.

### Features

* **connectors:** AAuth connectors with Cedar policy entities, and member policy attributes ([#115](https://github.com/introspection-org/introspection-rust-sdk/issues/115)) ([5e0507e](https://github.com/introspection-org/introspection-rust-sdk/commit/5e0507ebdf5cc49ed88885389379e18eb562302d))

## [0.26.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.25.0...v0.26.0) (2026-10-10)


### ⚠ BREAKING CHANGES

* **conversations:** `share_id` is removed from `ConversationListParams`, `ConversationItemListParams`, `ConversationItemGetParams` and `ConversationExportParams`. It was deprecated in 0.25.0 and ignored by the server, so dropping it from a struct literal changes nothing on the wire.

### Code Refactoring

* **conversations:** drop the share_id read parameter ([#113](https://github.com/introspection-org/introspection-rust-sdk/issues/113)) ([513450b](https://github.com/introspection-org/introspection-rust-sdk/commit/513450b0fe96a7c4f7f9f9b7295358341216e3a5))

## [0.25.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.24.0...v0.25.0) (2026-10-10)


### Features

* **shares:** tag shares, PATCH visible_from and issue shares ([#111](https://github.com/introspection-org/introspection-rust-sdk/issues/111)) ([1099360](https://github.com/introspection-org/introspection-rust-sdk/commit/1099360e5c8b41e4d7ee81b047d82d1e18d21ba7))

## [0.24.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.23.0...v0.24.0) (2026-10-07)


### ⚠ BREAKING CHANGES

* `RuntimeHandle::id()` returns `Option<Uuid>` (`None` for a slug handle), and `RunnerSource::Runtime` carries `runtime: StringOrUuid` in place of `runtime_id: Uuid`.

### Features

* open a runner by Runtime slug without listing; library-specific User-Agent ([#109](https://github.com/introspection-org/introspection-rust-sdk/issues/109)) ([5154acb](https://github.com/introspection-org/introspection-rust-sdk/commit/5154acbb6afc791ebbc8433a300bf6378bdd6d85))

## [0.23.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.22.0...v0.23.0) (2026-10-06)


### ⚠ BREAKING CHANGES

* **connections:** IntrospectionClient::events() and ::automations() return an owned handle (Events, Automations) instead of a reference, matching Runner. Method calls are unaffected; code that names the &Events / &Automations type must drop the reference.
* IntrospectionAPIError gains a variant and TaskCreate gains a field, so exhaustive matches and struct literals need updating.
* **automations:** `IntrospectionEventName` and `Event` have new variants (`AutomationTriggered`, `AutomationSkipped`), so exhaustive matches need arms for them, and `EventListParams` has new fields `automation_id` and `task_id` (literals built with `..EventListParams::new(..)` are unaffected).
* **members:** RunnerIdentity has a new field `metadata` (exhaustive struct literals need it; `RunnerIdentity { .., ..Default::default() }` is unaffected).
* **streaming:** reject incomplete output and resume nonterminal closes ([#102](https://github.com/introspection-org/introspection-rust-sdk/issues/102))
* **files:** FileListParams has a new field `metadata` (exhaustive struct literals need it; `FileListParams { .., ..Default::default() }` is unaffected).
* **files:** FileCreateText and FileUpload have new fields `tags` and `metadata` (exhaustive struct literals need them; `FileCreateText { .., ..Default::default() }` and the FileUpload constructors are unaffected).

### Features

* **automations:** add automations resource and automation event families ([#106](https://github.com/introspection-org/introspection-rust-sdk/issues/106)) ([89e8dc6](https://github.com/introspection-org/introspection-rust-sdk/commit/89e8dc6657c5a0e6852f550f4f4e6c179c808613))
* **connections:** one Data Plane surface on client and runner, with connections ([#108](https://github.com/introspection-org/introspection-rust-sdk/issues/108)) ([4950f5b](https://github.com/introspection-org/introspection-rust-sdk/commit/4950f5b5a76cb7868d66bd3dc7ee88832491e34a))
* **files:** accept tags and metadata on file create ([#100](https://github.com/introspection-org/introspection-rust-sdk/issues/100)) ([f65cbf2](https://github.com/introspection-org/introspection-rust-sdk/commit/f65cbf2438f890b2b7c0f5e246925eb0b215621d))
* **files:** filter file lists by metadata ([#103](https://github.com/introspection-org/introspection-rust-sdk/issues/103)) ([563366a](https://github.com/introspection-org/introspection-rust-sdk/commit/563366a58e27f116a39675346a9bec5ba7ed6325))
* **members:** member tags and metadata, and identity-asserted metadata ([#105](https://github.com/introspection-org/introspection-rust-sdk/issues/105)) ([a48b07d](https://github.com/introspection-org/introspection-rust-sdk/commit/a48b07de55dc63bea848b06d77d0cfadf2906884))
* native email-code sign-in, OAuth error mapping and TaskCreate.runtime_id ([#104](https://github.com/introspection-org/introspection-rust-sdk/issues/104)) ([0c2151e](https://github.com/introspection-org/introspection-rust-sdk/commit/0c2151ee24357bdcb88852c3094c0bf46e413957))
* **otel:** add log_event for custom events; track delegates to it ([#107](https://github.com/introspection-org/introspection-rust-sdk/issues/107)) ([177e015](https://github.com/introspection-org/introspection-rust-sdk/commit/177e0155af561935fae9f414a97add8c2329c64c))


### Bug Fixes

* **streaming:** reject incomplete output and resume nonterminal closes ([#102](https://github.com/introspection-org/introspection-rust-sdk/issues/102)) ([2aab7e5](https://github.com/introspection-org/introspection-rust-sdk/commit/2aab7e5e1e0b3b67dc4669795d8407ac78c0625e))

## [0.22.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.21.0...v0.22.0) (2026-09-29)


### Features

* add repository merges ([#98](https://github.com/introspection-org/introspection-rust-sdk/issues/98)) ([7965eb7](https://github.com/introspection-org/introspection-rust-sdk/commit/7965eb7238b3c1c6b9596acc070d7383397f39d9))
* **connectors:** custom-app search, OAuth discovery and authorize binding ([#97](https://github.com/introspection-org/introspection-rust-sdk/issues/97)) ([b954e1a](https://github.com/introspection-org/introspection-rust-sdk/commit/b954e1a839f22ba433ae097247d9aa8d36c87cfb))

## [0.21.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.20.0...v0.21.0) (2026-09-24)


### ⚠ BREAKING CHANGES

* **deps:** the otel feature now uses opentelemetry, opentelemetry_sdk and opentelemetry-otlp 0.33, which appear in the public API (e.g. to_otel_value, otel::testing); callers must move to 0.33 with it.

### Miscellaneous Chores

* **deps:** bump OpenTelemetry to 0.33 ([#95](https://github.com/introspection-org/introspection-rust-sdk/issues/95)) ([bf6158e](https://github.com/introspection-org/introspection-rust-sdk/commit/bf6158e6a47cf1968a115660b4a87fc3c5a9214b))

## [0.20.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.19.0...v0.20.0) (2026-09-24)


### ⚠ BREAKING CHANGES

* Repositories::new takes (cp_http, dp_http), and Repository has two new fields (exhaustive struct literals need them).

### Features

* add repository commits ([#94](https://github.com/introspection-org/introspection-rust-sdk/issues/94)) ([ee85c09](https://github.com/introspection-org/introspection-rust-sdk/commit/ee85c0901446f2ee4d8a84649934b15b0537b02f))
* add repository contents ([#92](https://github.com/introspection-org/introspection-rust-sdk/issues/92)) ([170efa1](https://github.com/introspection-org/introspection-rust-sdk/commit/170efa1cc1ddf84578791fb7e73893d9de3ac65e))

## [0.19.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.18.0...v0.19.0) (2026-09-17)


### Features

* make every resource namespace's `new` public ([#88](https://github.com/introspection-org/introspection-rust-sdk/issues/88)) ([ae037cd](https://github.com/introspection-org/introspection-rust-sdk/commit/ae037cd60d680cc422ee5449e2b7e0faf2e62d34))

## [0.18.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.17.0...v0.18.0) (2026-09-17)


### ⚠ BREAKING CHANGES

* `Repositories::list(project)` is now `Repositories::list(&RepositoryListParams)`, and every `*ListParams` struct has a new `filters` field (exhaustive struct literals need `filters: None`; `..Default::default()` is unaffected).

### Features

* repositories list takes params with a slug filter; every list accepts passthrough filters ([#86](https://github.com/introspection-org/introspection-rust-sdk/issues/86)) ([5fdb8fa](https://github.com/introspection-org/introspection-rust-sdk/commit/5fdb8fad469a95c5abce1cee173a3f492669074b))

## [0.17.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.16.0...v0.17.0) (2026-09-17)


### Features

* add repositories lookup, runtime yank fields, and a public HttpClient::from_parts ([#84](https://github.com/introspection-org/introspection-rust-sdk/issues/84)) ([6d4244c](https://github.com/introspection-org/introspection-rust-sdk/commit/6d4244c8a42d90d564bd0747c1c18feceb3a5972))

## [0.16.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.15.0...v0.16.0) (2026-09-02)


### Features

* Add span annotations and project labels client ([#82](https://github.com/introspection-org/introspection-rust-sdk/issues/82)) ([c782754](https://github.com/introspection-org/introspection-rust-sdk/commit/c782754bf473cd42dbae679b8b8d4421c391fd14))
* **connectors:** support Pipedream applications ([#83](https://github.com/introspection-org/introspection-rust-sdk/issues/83)) ([8ab6bdf](https://github.com/introspection-org/introspection-rust-sdk/commit/8ab6bdfda964e5c9f1faafcc9318f0b9f7a433ef))
* **conversations:** add conversation metadata ([#81](https://github.com/introspection-org/introspection-rust-sdk/issues/81)) ([1339423](https://github.com/introspection-org/introspection-rust-sdk/commit/13394236881f2c698a2021c28d3f9fa6e2296976))


### Bug Fixes

* expose conversation task titles ([8d53a99](https://github.com/introspection-org/introspection-rust-sdk/commit/8d53a99936ea8628cd5054f120b90f243c4bc5fc))

## [0.15.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.14.0...v0.15.0) (2026-08-13)


### Features

* **connectors:** add connectors, connections, and the install link ([#74](https://github.com/introspection-org/introspection-rust-sdk/issues/74)) ([550be0c](https://github.com/introspection-org/introspection-rust-sdk/commit/550be0c5a3d5ffe3196c8639de1f754da0ccdd91))


### Bug Fixes

* expose file tags, drop identity_key, and make the contract test able to run again ([#78](https://github.com/introspection-org/introspection-rust-sdk/issues/78)) ([b2d27e7](https://github.com/introspection-org/introspection-rust-sdk/commit/b2d27e7fb56486d95948ef9edb604d443c6a77bb))

## [0.14.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.13.0...v0.14.0) (2026-08-10)


### ⚠ BREAKING CHANGES

* IntrospectionClient::projects and ::repositories are removed, along with experiments and recipes create/update/delete and the Project, Repository, RecipeCreate, RecipeUpdate, ExperimentCreate and ExperimentUpdate types.

### Features

* add complete conversation export parity ([#71](https://github.com/introspection-org/introspection-rust-sdk/issues/71)) ([5573519](https://github.com/introspection-org/introspection-rust-sdk/commit/55735190538629eacfb92a6764bbcfd88143798d))
* bring the SDK to parity on the runner-plane boundary ([#66](https://github.com/introspection-org/introspection-rust-sdk/issues/66)) ([9a6e9ae](https://github.com/introspection-org/introspection-rust-sdk/commit/9a6e9ae5609416d20e85b43d38de1b921d6c6dde))


### Bug Fixes

* drop the runtime-list filter the API never accepted ([#69](https://github.com/introspection-org/introspection-rust-sdk/issues/69)) ([1c04aa6](https://github.com/introspection-org/introspection-rust-sdk/commit/1c04aa6ea0d4d8e3116a74f21df3ae536c4633e2))
* export the field types exported structs expose, and cut the README to the basics ([#72](https://github.com/introspection-org/introspection-rust-sdk/issues/72)) ([04898e8](https://github.com/introspection-org/introspection-rust-sdk/commit/04898e89d993ccf13d8b72d0cb182bede99d72e4))

## [0.13.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.12.0...v0.13.0) (2026-08-07)


### Features

* **conversations:** add summary resources and agent selection ([#64](https://github.com/introspection-org/introspection-rust-sdk/issues/64)) ([a759233](https://github.com/introspection-org/introspection-rust-sdk/commit/a759233cc042d4dc8177393cc657f50d8fd44ba2))

## [0.12.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.11.0...v0.12.0) (2026-08-05)


### ⚠ BREAKING CHANGES

* **conversations:** `ConversationItem`, `ConversationItemList` and `Conversation` are removed. All three conversation reads — the summary list, the item list and the item detail — now return `GenAiSpan` / `GenAiSpanList`. The message-family `include` values and `span_attributes` are removed from `ConversationItemInclude`, leaving `events` and `resource_attributes`.

### Features

* **conversations:** return GenAI spans, drop the flat ConversationItem ([#61](https://github.com/introspection-org/introspection-rust-sdk/issues/61)) ([ff827c7](https://github.com/introspection-org/introspection-rust-sdk/commit/ff827c7aab1533c869f6fdaa5d88e4f80923b46e))

## [0.11.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.10.0...v0.11.0) (2026-07-29)


### Features

* add conversation item pagination ([#54](https://github.com/introspection-org/introspection-rust-sdk/issues/54)) ([b3ec069](https://github.com/introspection-org/introspection-rust-sdk/commit/b3ec0699826248ade5f566413c959ee58248b482))
* **otel:** trace OpenAI embedding usage ([#53](https://github.com/introspection-org/introspection-rust-sdk/issues/53)) ([0487898](https://github.com/introspection-org/introspection-rust-sdk/commit/0487898565afd77db191d295719d4fbcc4303d59))

## [0.10.0](https://github.com/introspection-org/introspection-rust-sdk/compare/v0.9.0...v0.10.0) (2026-07-29)


### Features

* route tasks to a named dev server via INTROSPECTION_DEV_TARGET ([#51](https://github.com/introspection-org/introspection-rust-sdk/issues/51)) ([a101611](https://github.com/introspection-org/introspection-rust-sdk/commit/a101611070bc244bed6455200fd85f0962880f08))

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
