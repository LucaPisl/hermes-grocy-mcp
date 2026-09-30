# Hermes Grocy MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a lightweight Rust stdio MCP that gives Hermes useful household reads and writes through Grocy's API, with protected desktop-wallet enrollment and understandable setup.

**Architecture:** A reviewed operation catalog feeds MCP discovery, validation and a single bounded HTTP client. Each call obtains its private profile from the explicitly selected wallet; neither the server nor Hermes configuration persists a plaintext credential. CLI enrollment and runtime share these boundaries, while a disposable Docker fixture supplies synthetic integration data.

**Tech Stack:** Rust 2024; `rmcp` 1.5, Tokio, serde/serde_json, schemars, reqwest with rustls and default features disabled, url, rust_decimal, secrecy/zeroize, clap, dialoguer, base64, and libsecret 0.9 with matching GIO/GLib bindings. System libsecret handles Secret Service cryptography; no GTK UI or custom cryptography. Cargo.lock pins resolved compatible versions. Test-only HTTP support uses axum; the fixture runner uses Python's standard library and Docker CLI.

**Spec:** [Approved design](../specs/2026-10-01-hermes-grocy-mcp-design.md).

## Global Constraints

- Linux desktop Secret Service v1 initially; one executable, stdio only, AGPL-3.0-only. Unsupported platforms fail with an actionable backend message.
- Every Grocy request stays on the enrolled origin and deployment prefix ending in `/api`, including setup, doctor, files and exports. HTTPS with verified TLS; HTTP requires explicit enrollment of a literal loopback development destination.
- Disable redirects, ambient proxies, cookies, compression and automatic write retries. Connect timeout 10 seconds; total request timeout 30 seconds; JSON/text limit 2 MiB; binary limit 8 MiB; default page 100, maximum 500.
- Full household reads and writes, including deletes and supported undo; optional read-only enrollment omits mutation tools. Exclude account/security administration, public sharing links, external barcode lookups, printer actions and arbitrary API passthrough.
- Selected provider only, unique D-Bus owner per secret operation, encrypted IPC mandatory, runtime never unlocks or creates wallets. Protection uncertainty has native help and a distinct explicit default-off backend-risk override.
- Private profile is one encrypted wallet item. Only provider identity and a nonsecret profile label enter restricted preferences; Hermes configuration contains executable/profile/provider arguments. No credential files, environment fallback, command-line secret arguments, telemetry or household cache.
- Actual data in JSON text and structured results; stable redacted errors. Never replay an uncertain write. Report successful writes with failed readback as completed writes with incomplete verification.
- Build output, fixture state, synthetic keys, captures and local reports remain outside tracked source. Preserve the contributor's configured public Git identity; no assistant attribution. Do not touch existing household data or unrelated Docker resources.
- Implement inline in this session, preserving the user's execution preference. Use coherent task commits, focused checks and one final review; no repeated review rounds or test-count target. Publication and real enrollment follow verification.

## Review Focus

1. An Authelia redirect or misleading encoded path must never forward the key or leave the enrolled API prefix (Task 1).
2. A wallet restarts, changes owner, is locked, or negotiates plaintext: fail safely without fallback, unlocking, or secret transfer (Task 2).
3. Two products share a name, or a quantity uses a different unit: return candidates or require a verified conversion, never guess (Task 3).
4. Grocy accepts a write but its response or verification is lost: distinguish uncertain dispatch from completed writes and never replay (Tasks 1 and 4).
5. Setup is rerun beside an existing Hermes entry: reuse choices and refuse a conflicting entry without damaging other configuration (Task 6).

## File Map and Shared Types

| Files | Responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/main.rs`, `src/model.rs`, `src/error.rs`, `LICENSE`, `.gitignore` | Build, command entry, shared nonsecret types and sanitized errors |
| `src/client/{mod,destination,response}.rs`, `tests/client_boundary.rs` | Sole HTTP boundary, streaming limits and dispatch outcome |
| `src/credentials/{mod,providers,secret_service,protection,preferences}.rs`, `tests/credentials.rs`, `tests/support/secret_service.rs` | Explicit provider discovery, libsecret adapter, protection evidence and preferences |
| `src/catalog/{mod,entities,operations,schemas}.rs`, `src/tools/{mod,records,validation}.rs`, `tests/catalog.rs` | Reviewed household policy, fields, CRUD and decimal/unit validation |
| `src/tools/{stock,shopping,recipes,chores,tasks,batteries,files,exports}.rs`, `tests/workflows.rs` | Domain operations, readbacks and binary/export handling |
| `src/mcp.rs`, `tests/mcp_protocol.rs`, `tests/support/{mod,fixtures}.rs` | MCP registration, annotations, response representation and synthetic harness |
| `src/cli.rs`, `src/setup/{mod,wizard,help,hermes}.rs`, `tests/setup.rs` | Guided CLI, recovery, diagnostics and preserving Hermes registration |
| `tests/docker_contract.rs`, `tests/support/api_proxy.rs`, `scripts/docker_fixture.py`, `docs/{setup,api-coverage,compatibility}.md`, `README.md`, `SECURITY.md`, `.github/workflows/ci.yml` | Disposable contract run, public guidance, coverage and verification |

Task 1 defines `Result<T> = std::result::Result<T, AppError>`, `AccessMode::{ReadOnly, Full}`, `TransportPolicy::{HttpsOnly, LoopbackDevelopment}`, `ProfileSelection { profile: String, provider: ProviderIdentity }`, and `ProviderIdentity { bus_name: String, executable: PathBuf }`. `PrivateProfile` owns a `SecretString` API key, API base, access mode, timezone/defaults, optional CA path and enrollment choices; it has no derived Debug or public serialization. Explicit wallet encode/decode functions use zeroizing buffers. `Page { offset: usize, limit: usize }`, `RecordId(u64)` and `Decimal` quantities are shared validated types. Paged data identifies `items`, `has_more` and `next_offset`; include `total` only when actually known.

`ToolOutput { data: serde_json::Value, write: Option<WriteReport>, attachment: Option<Attachment> }` defines the domain result. `WriteReport { status: WriteStatus, verification: Verification, target: Value, receipt: Value }` uses `WriteStatus::Completed` and `Verification::{Confirmed, Incomplete, Unavailable}`; uncertain dispatch is an `AppError`, not a completed report. `Attachment` owns bounded bytes and a validated MIME type. Public error fields contain code, generic message and nonsensitive recovery identifiers only.

---

### Task 1: Build the API-only transport boundary

**Files:** Create build/shared/client files and `tests/client_boundary.rs` from the map. Add the complete AGPL-3.0 license and ignore build/local outputs in this first product commit.

**Interfaces:** `ApiBase::parse(input: &str, policy: TransportPolicy) -> Result<ApiBase>`; `ApiBase::route(template: &'static str, params: &BTreeMap<String, String>) -> Result<Url>`; `ApiClient::new(profile: PrivateProfile) -> Result<ApiClient>`; `async ApiClient::request(&self, request: ApiRequest) -> Result<ApiResponse>`. Internal `ApiRequest` contains a static reviewed route template, `HttpMethod`, separately encoded path/query parameters, `RequestBody::{Empty, Json(Value), Binary(Vec<u8>)}`, and `ResponseKind::{Json, Text, Binary}`. `ApiResponse` contains status, decoded bounded body and validated content type. No unchecked request interface is exposed to MCP.

- [ ] **Write `api_boundary` and `write_dispatch_boundary` parameterized tests:**
  ```rust
  assert_eq!(https_base("https://grocy.example/household").path(), "/household/api");
  assert_eq!(https_base("https://grocy.example/household/api/").path(), "/household/api");
  assert!(rejects_bases_with_userinfo_query_fragment_traversal_or_encoded_separators());
  assert!(loopback_http_requires_explicit_development_policy());
  assert_eq!(redirect_fixture.received_requests(), 1);
  assert_eq!(redirect_target.received_requests(), 0);
  assert!(limits_refuse_bytes(2 * 1024 * 1024 + 1, 8 * 1024 * 1024 + 1));
  assert!(compression_is_refused_and_environment_proxy_is_unused());
  assert_eq!(dispatched_write_timeout.code(), "UNKNOWN_WRITE_OUTCOME");
  assert_eq!(write_fixture.attempts(), 1);
  assert!(!upstream_error.exposed_text().contains("synthetic-private-marker"));
  ```
- [ ] **Run red:** `cargo test --test client_boundary`; expect missing implementation or failing boundary assertions.
- [ ] **Implement these interfaces:** reject ambiguous URL encodings before normalization; encode substitutions as individual segments and recheck final origin/prefix. Disable reqwest retries/redirects/proxies/compression; stream bodies with exact caps. Mark writes potentially dispatched before polling send; classify transport/cancellation uncertainty conservatively. Retain no raw upstream body in errors. Custom CA parsing uses the same verified TLS policy. Capture one invalid-certificate fixture in this suite.
- [ ] **Run green:** `cargo test --test client_boundary`; expect all cases passing.
- [ ] **Commit:** `feat: enforce bounded API-only Grocy transport` with only this task's source, tests, license and build files.

### Task 2: Protect profiles with explicit desktop-wallet selection

**Files:** Create credential modules, `tests/credentials.rs` and test-only isolated D-Bus fake provider support.

**Interfaces:** `CredentialStore: Send + Sync` exposes `load(selection: ProfileSelection) -> BoxFuture<'static, Result<PrivateProfile>>`, `put(selection: ProfileSelection, profile: PrivateProfile, consent: EnrollmentConsent) -> BoxFuture<'static, Result<()>>`, and `forget(selection: ProfileSelection) -> BoxFuture<'static, Result<()>>`. `DesktopStore` implements it. `discover_providers() -> Result<Vec<ProviderCandidate>>`; `inspect_protection(identity: &ProviderIdentity) -> ProtectionEvidence`; `PreferenceStore::{load, save}` accepts only profile labels/provider identities. `EnrollmentConsent` distinguishes verified protection, acknowledged unverified protection and explicitly accepted unsafe backend storage.

- [ ] **Write `selected_wallet_only` and `protected_preferences` tests:**
  ```rust
  assert_eq!(fake_bus.last_destination(), selected_unique_owner);
  assert_eq!(fake_bus.fallback_calls(), 0);
  assert_eq!(locked_runtime.code(), "WALLET_LOCKED");
  assert_eq!(fake_bus.runtime_unlock_or_create_calls(), 0);
  assert_eq!(plaintext_session.code(), "UNENCRYPTED_SECRET_SESSION");
  assert_eq!(fake_bus.plaintext_secret_transfers(), 0);
  assert!(owner_restart_fails_without_using_replacement_owner());
  assert_eq!(protection_for_encrypted_kwallet_header(), ProtectionEvidence::Unverified);
  assert!(protection_probe_refuses_symlinks_and_nonowned_files());
  assert_eq!(preferences_mode(), 0o600);
  assert!(!preferences_text().contains("synthetic-key-or-url"));
  ```
- [ ] **Run red:** `cargo test --test credentials`; expect missing adapter or failed policy assertions.
- [ ] **Implement:** use GIO to list names and resolve the selected live unique owner, retaining executable identity for subsequent operations. Open libsecret `Service::open_sync(Service::static_type(), Some(unique_owner), OPEN_SESSION, cancellable)` on a worker thread, and require `session_algorithms() == "dh-ietf1024-sha256-aes128-cbc-pkcs7"` before any Get/SetSecret. Use cancellable deadlines, existing default alias/collection, exact application+profile attributes and no auto-unlocking convenience lookup. Runtime loads only unlocked matching items; setup may explicitly request native unlock. Replacements use the service's single-item replace capability; interruption is reported honestly. Bound protection probes to owned regular files and known headers; KWallet format stays unverified. Preferences use no-follow reads and atomic mode-0600 writes in a mode-0700 app directory. Never expose library error details or silently activate/fallback. Synthetic provider exists only under test modules.
- [ ] **Run green:** `cargo test --test credentials`; expect passing fake-bus and filesystem-policy cases without accessing real wallet secrets.
- [ ] **Commit:** `feat: store profiles in the selected protected desktop wallet`.

### Task 3: Define reviewed household records and quantity rules

**Files:** Create catalog modules, records/validation tools and `tests/catalog.rs`; start `docs/api-coverage.md` with upstream method/path mapping and explicit exclusions.

**Interfaces:** `catalog::entities() -> &'static [EntitySpec]`; `catalog::operations(mode: AccessMode) -> Vec<OperationSpec>`; `catalog::validate(name: &str, args: Value, mode: AccessMode) -> Result<ValidatedCall>`. Specs carry schemas, route/method, read/write/destructive policy and verification strategy. `async records::run(client: &ApiClient, call: &ValidatedCall) -> Result<ToolOutput>`; `resolve_record(rows: &[Value], selector: &RecordSelector) -> Result<RecordId>`; `convert_quantity(amount: Decimal, from: RecordId, stock_unit: RecordId, conversions: &[Conversion]) -> Result<Decimal>`. Define `RecordSelector::{Id, ExactName}` and positive decimal `Conversion` factors; never resolve names by substring.

- [ ] **Write `household_catalog_and_validation`:**
  ```rust
  assert!(read_only_operations().iter().all(|op| !op.mutates));
  assert!(catalog_excludes_accounts_keys_sessions_permissions_and_readonly_view_writes());
  assert_eq!(resolve_duplicate_name().code(), "AMBIGUOUS_RECORD");
  assert_eq!(convert("500", grams, kilograms, "0.001"), decimal("0.500"));
  assert_eq!(missing_conversion().code(), "UNIT_CONVERSION_REQUIRED");
  assert!(negative_quantities_and_invalid_dates_are_rejected());
  assert_eq!(Page::default().limit, 100);
  assert!(Page::new(0, 501).is_err());
  assert!(stock_unit_change_requires_valid_supported_conversion());
  ```
- [ ] **Run red:** `cargo test --test catalog`; expect catalog/validation failures.
- [ ] **Implement:** reviewed schemas and finite entity CRUD for products, barcodes, locations, groups, units/conversions, shopping lists/items/locations, recipes/ingredients/nestings, meal plans/sections, chores, tasks/categories, batteries, equipment, userfields/entities/objects. Read-only views remain readable. Validate known editable fields against reviewed upstream definitions; reject server-owned/unknown fields. Userfield target entities must also be allowed. Add record description/name lookup tools, restricted assignment-user ID/name projection, deterministic pagination and explicit ambiguity candidates. Validate product references/units/location metadata and only Grocy-supported unit-change procedures. Do not fetch system configuration or permissions to guess capabilities.
- [ ] **Run green:** `cargo test --test catalog`; expect all shared policy cases passing. Review one product CRUD schema before expanding the same catalog pattern across other entities.
- [ ] **Commit:** `feat: add reviewed household records and unit validation`.

### Task 4: Implement domain workflows, files and honest write results

**Files:** Create domain modules from the map, `tests/workflows.rs`; complete operation mapping in `docs/api-coverage.md`.

**Interfaces:** Each domain exposes `async run(client: &ApiClient, call: &ValidatedCall) -> Result<ToolOutput>`. `tools::execute(client: &ApiClient, call: ValidatedCall) -> Result<ToolOutput>` dispatches finite catalog entries. `async verify_write(client: &ApiClient, receipt: ApiResponse, target: VerificationTarget) -> Result<ToolOutput>` preserves confirmed dispatch independently of readback. `VerificationTarget::{Readback(ApiRequest), Unavailable}` selects one reviewed bounded read or explicitly unavailable verification. `Attachment` is created only after decoded-size/MIME validation. No upload/download interface accepts a local path or external URL.

- [ ] **Write `workflow_results_and_files`:**
  ```rust
  assert_eq!(volatile_result.data["due_products"][0]["id"], 7);
  assert!(successful_204.write.unwrap().status == WriteStatus::Completed);
  assert!(failed_readback.write.unwrap().verification == Verification::Incomplete);
  assert_eq!(accepted_write_fixture.attempts(), 1);
  assert!(receipts_preserve_object_booking_transaction_and_execution_ids());
  assert_eq!(uploaded_body, synthetic_binary_bytes);
  assert_eq!(downloaded_bytes, synthetic_binary_bytes);
  assert!(filename_traversal_and_oversize_base64_are_rejected_before_request());
  assert!(api_ical_is_text_calendar_and_shopping_export_contains_actual_items());
  ```
- [ ] **Run red:** `cargo test --test workflows`; expect unimplemented actions/results.
- [ ] **Implement:** stock reads/actions/entry edits, copy/merge, barcode variants, bookings/transactions and undo; shopping additions/removals/clear/missing/expired/overdue; recipe fulfillment/copy/consume/shopping additions and transaction undo; chores executions/undo/assignment recalculation/merge; battery charge/undo; task complete/undo and rescheduling through record updates. Inventory allows a new amount of zero; add/consume/transfer/open require positive amounts. Return one suitable bounded readback and distinguish incomplete verification. For grouped stock results, preserve groups and page each reported group explicitly. Files use household groups `equipmentmanuals`, `recipepictures`, `productpictures`, `userfiles`; exclude account avatar mutation. Use Grocy-required base64 filenames and binary PUT bodies; validate image signatures/types, otherwise embedded bounded resources. Export iCal through `/calendar/ical`; produce a shopping-list data export from API records, never call printer endpoints. Unsupported-version operations return explicit capability errors, with no fabricated fallback writes.
- [ ] **Run green:** `cargo test --test workflows`; expect passing domain/result/binary cases. Coverage inventory accounts for every reviewed household upstream route and excluded operation.
- [ ] **Commit:** `feat: implement household actions files and verified write receipts`.

### Task 5: Expose useful and discoverable MCP tools

**Files:** Create `src/mcp.rs`, `tests/mcp_protocol.rs` and synthetic support modules; wire `serve` in `src/main.rs`.

**Interfaces:** `McpServer::new(store: Arc<dyn CredentialStore>, selection: ProfileSelection, mode: AccessMode) -> McpServer`; `async mcp::serve_stdio(store: Arc<dyn CredentialStore>, selection: ProfileSelection) -> Result<()>`; `to_mcp_result(result: Result<ToolOutput>) -> rmcp::model::CallToolResult`. Server keeps only selection/mode/store and a process-local mutation mutex, loads a fresh profile per call and advertises the Task 3 catalog through `ServerHandler::list_tools/call_tool`.

- [ ] **Write `protocol_returns_data_and_policy`:**
  ```rust
  assert_eq!(json_text_data(&inventory_result), inventory_result.structured_content.unwrap());
  assert_eq!(inventory_data["items"][0]["id"], 7);
  assert!(delete_tool.annotations.unwrap().destructive_hint == Some(true));
  assert!(readonly_discovery_omits_every_mutation());
  assert!(unknown_tool_result.is_error == Some(true));
  assert!(error_response_omits_key_private_url_and_raw_upstream_body());
  assert!(parallel_mutations_are_serialized_and_reads_remain_bounded());
  assert!(descriptions_explain_units_dates_paging_and_uncertain_write_recovery());
  ```
- [ ] **Run red:** `cargo test --test mcp_protocol`; expect discovery/response failures using SDK duplex transport, not a production plaintext profile switch.
- [ ] **Implement:** publish record CRUD/lookup/description tools and named finite domain operations with specific schemas, units and generic examples; truthful read-only/destructive/idempotency/open-world annotations. Register file/export tools with text plus image/resource content. Keep stdout protocol-only, sanitized diagnostics on stderr, bounded input/upload schemas and no request/body logging. Check the profile's current access mode again on every call; fail safely if re-enrollment changes advertised mode. Cancellation must not spawn a replay.
- [ ] **Run green:** `cargo test --test mcp_protocol`; expect actual SDK discovery/calls and result equivalence passing.
- [ ] **Commit:** `feat: expose data-rich household tools over stdio MCP`.

### Task 6: Make enrollment and Hermes connection understandable

**Files:** Create CLI/setup modules, `tests/setup.rs`, `docs/setup.md`; extend command dispatch in `src/main.rs`.

**Interfaces:** `async setup::run(options: SetupOptions, ui: &mut dyn SetupUi, store: Arc<dyn CredentialStore>) -> Result<SetupOutcome>`; `async setup::doctor(selection: ProfileSelection, store: Arc<dyn CredentialStore>) -> Result<DoctorReport>`; `hermes::render_config(executable: &Path, selection: &ProfileSelection) -> Result<String>`; `async hermes::connect(executable: &Path, selection: &ProfileSelection) -> Result<ConnectOutcome>`. `SetupUi` provides visible input, hidden secret input, defaulted choices, confirmation and messages; tests use scripted answers. Outcomes expose status/nonsecret recovery instructions, not a profile or key.

- [ ] **Write `guided_setup_and_hermes_preservation`:**
  ```rust
  assert_eq!(new_setup.required_private_inputs(), ["instance URL", "API key"]);
  assert!(kde_provider_is_recommended_without_assuming_kwallet_password_protection());
  assert!(unprotected_storage_has_native_steps_and_default_off_override());
  assert!(wallet_or_network_retry_preserves_already_entered_answers());
  assert!(rerun_reuses_existing_profile_selection_and_defaults());
  assert!(cancel_before_final_review_writes_no_profile());
  assert_eq!(conflicting_hermes_entry.code(), "HERMES_ENTRY_CONFLICT");
  assert_eq!(unrelated_hermes_configuration_before, unrelated_hermes_configuration_after);
  assert!(rendered_block_contains_no_key_server_url_or_private_metadata());
  ```
- [ ] **Run red:** `cargo test --test setup`; expect wizard/configuration failures.
- [ ] **Implement:** `setup`, `serve`, `doctor`, `wallets`, `print-config`, `connect-hermes`, `forget`. Choose the desktop provider/default wallet; show native steps for unavailable, locked, unverified or unprotected storage. Explain a dedicated Grocy user, household permissions, Manage API keys, upstream identity-provider 2FA and model/history privacy. Ask URL and masked key, discover version/time/household defaults via API only, reuse prior selections, and show one final review before storage/replacement. Timezone comes from server metadata or local IANA configuration; offer detected choices, with manual search only as recovery. Clarify TLS/custom-CA/Authelia/migration errors without root access. Inspect the installed Hermes CLI preserving-writer interface read-only, then use its conflict-safe writer and discovery check; if unavailable print a credential-free block and exact installation steps. Do not replace YAML wholesale. Forget only the matching wallet item/preferences and explain server-key revocation.
- [ ] **Run green:** `cargo test --test setup`; expect scripted first-run/retry/re-enrollment/conflict flows passing. Walk through the rendered help once as an unassisted user.
- [ ] **Commit:** `feat: guide protected enrollment and Hermes registration`.

### Task 7: Verify the complete contract and prepare public delivery

**Files:** Create Docker contract/proxy/runner, README, SECURITY, compatibility notes and CI; update setup/coverage docs and the plan checkboxes.

**Interfaces:** `scripts/docker_fixture.py` provides `start`, `test`, `stop` using one owned manifest in a temporary directory; start records the LinuxServer image digest, creates only tagged disposable resources and obtains a synthetic key through independent fixture bootstrap. `cargo test --test docker_contract -- --ignored` reads the synthetic fixture manifest in test code only. No released binary accepts fixture credentials. `ApiOnlyProxy` exposes root `/api` and deployment-prefix `/household/api`, rejects everything else and keeps only nonsensitive fixture request counters.

- [ ] **Write `docker_household_contract`:**
  ```rust
  assert!(sdk_discovery_and_calls_return_real_synthetic_inventory());
  assert!(setup_and_doctor_pass_through_api_only_proxy());
  assert!(crud_actions_and_supported_undo_pass_for_every_household_domain());
  assert_eq!(file_round_trip, uploaded_synthetic_bytes);
  assert!(ical_and_shopping_export_are_useful());
  assert_eq!(proxy.non_api_request_count(), 0);
  assert!(deployment_prefix_is_preserved());
  assert!(readonly_profile_cannot_mutate());
  ```
- [ ] **Run red:** `python scripts/docker_fixture.py start` then `python scripts/docker_fixture.py test`; expect assertions to catch any route/schema/workflow gap. Bootstrap Grocy separately, including migrations if needed, before exposing the API-only proxy. Bind host ports to loopback and keep all state outside source.
- [ ] **Complete contract integration:** one scenario creates its own fixtures (units/locations/products, shopping, recipes/meal plan, chore/task/battery, equipment/custom data), exercises meaningful CRUD/action/undo per domain, and cleans up only its resources. Correct actual API-version mismatches and update the coverage table; no smoke-test claim in place of domain coverage. Record image digest and version in public compatibility docs. Add practical install/build/enroll/Hermes instructions, protection limits, no cross-client revision guarantee and uncertain-write recovery. CI runs fmt/clippy/focused tests; Docker remains an explicit contract command.
- [ ] **Run green and final checks once:** `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`; `python scripts/docker_fixture.py test`; `cargo build --release --locked`. Run one RustSec dependency check plus a tracked-source/history privacy inspection for secrets/private names/hosts/addresses and accidental generated artifacts. Inspect CLI help and release discovery. Fix concrete findings and rerun only affected checks.
- [ ] **Perform one final spec review:** confirm API-only routing, all-domain coverage, wallet owner/IPC policy, correct results/write recovery and unassisted setup. Record actual tested limitations rather than claiming unsupported capabilities. Run `python scripts/docker_fixture.py stop` even after failure; never prune unrelated resources.
- [ ] **Commit:** `docs: document verified Grocy compatibility and secure setup`. Deliver the build location, meaningful verification results and any limitations. Publish only sanitized source/docs/tests using the contributor's identity after the verified result is concrete; real-instance enrollment requires the human to enter their hidden key locally.

## Planning Self-Review

One review maps every spec section to Tasks 1–7, checks interface/type consistency and assigns all five Review Focus cases to named tests. The first release retains full household scope, while accounts, external lookup, public sharing and printers are explicit exclusions. The libsecret adapter uses its documented configurable service name and session-algorithm inspection; no pure-Rust provider workaround or cryptographic fork is required. Implementation will use the installed OS library, so README must state that small runtime dependency. No additional preference questions are needed.

Primary implementation references: [libsecret Rust Service API](https://docs.rs/libsecret/0.9.0/libsecret/struct.Service.html), [libsecret session algorithms](https://gnome.pages.gitlab.gnome.org/libsecret/method.Service.get_session_algorithms.html), and the upstream API/Docker/SDK sources linked in the design.
