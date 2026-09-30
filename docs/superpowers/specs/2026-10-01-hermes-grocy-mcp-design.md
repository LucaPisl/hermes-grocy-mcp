# Hermes Grocy MCP: design

Date: 2026-10-01. Status: written design for human review; implementation has not started.

## Agreed outcome

Build a lightweight Rust stdio MCP for a self-hosted Grocy instance. Hermes must receive useful data and support full household reads and writes: creation, updates, deletion, and domain actions. Credentials and private connection information belong in the selected desktop wallet. Setup must work for a person without an LLM. Every MCP request to Grocy, including enrollment and diagnostics, goes through its API. Test against a disposable local Docker instance, not an existing household or the public demo. Maintain a clean AGPL-3.0-only repository using the contributor's existing public Git identity, without assistant attribution or personal artifacts.

The earlier conversational direction is approved. This document is the architectural review checkpoint before the implementation plan. No further preference questions are needed to define the first release.

## Approach and boundaries

Choose one Rust executable using the official `rmcp` SDK and local stdio transport. Reusing an existing server would preserve its integration complexity; a fresh Python implementation would shorten initial development but retain interpreter/environment management. The selected Rust approach favors distribution and low runtime overhead, with a small shared API client and credential boundary. Rust is not a substitute for correct destination, quantity, and mutation policies.

Version one targets Linux desktop Secret Service implementations, particularly KDE KWallet/KSecretService and GNOME Keyring. It does not introduce an HTTP MCP listener, account administration, a database, background synchronization, telemetry, external product lookups, or a separate calendar integration. Other operating systems receive a clear unsupported-credential-backend error until a protected native backend is implemented and verified.

## Components and data flow

- `cli`: `setup`, `serve`, `doctor`, `wallets`, `print-config`, `connect-hermes`, and `forget`. Only local CLI commands enroll/remove profiles or select a wallet.
- `credentials`: provider discovery, protected profile serialization, encrypted Secret Service communication, and bounded wallet-protection evidence. OS integration is behind a narrow trait so the application can use a synthetic provider in tests without a production plaintext bypass.
- `client`: the sole Grocy HTTP entry point, owning destination validation, authentication, TLS, response limits, errors, and write-outcome classification.
- `catalog` and `tools`: reviewed entity/operation definitions, typed argument validation, domain actions, pagination, and human-oriented MCP descriptions.
- `mcp`: protocol registration, annotations, and consistent data/error responses.
- `setup`: guided enrollment, local defaults and help, final review, and credential-free Hermes registration.

At each operation, load the encrypted profile from the explicitly selected provider, validate the input and operation policy, make bounded API requests, then return useful results. Avoid retaining credentials globally or persisting household responses. No tool can supply an arbitrary URL, HTTP header, HTTP method, filesystem path, or secret-store destination.

## API-only destination policy

Accept an HTTPS instance URL or API URL and normalize it to one enrolled API base ending in `/api`. Support an explicit deployment prefix, such as `/grocy/api`, without dropping that prefix. Reject URL credentials, query/fragment components, traversal, ambiguous encodings, and encoded separators. Construct subsequent paths from reviewed route templates and individually encoded parameters. Preserve the enrolled origin and API prefix on every request.

Setup, version checks, reads, writes, files, iCalendar exports, and diagnostics use that same client. Never scrape a frontend page, follow a calendar-sharing link, fetch an attachment URL outside the API, or request Grocy's root route. The user-facing setup help may explain where to click in the web UI; the MCP does not perform those browser actions.

Use verified TLS, with an optional explicitly selected trusted CA bundle. Disable redirects, ambient proxy discovery, verbose connection logging, cookies, and automatic write retries. Allow HTTP only for an explicitly enrolled literal loopback destination used for local development/testing; never generalize that exception to private networks or arbitrary hostnames. Production self-hosted connections use HTTPS.

A reverse-proxy login redirect returns a sanitized actionable error; the client does not attempt an Authelia login or move outside `/api`. If the API needs database migration, explain that an administrator must complete Grocy's normal upgrade procedure outside the MCP. Do not perform the root-page visit.

Initial limits: 10-second connect timeout, 30-second total request timeout, 2 MiB JSON/text responses and 8 MiB binary files. Enforce limits while streaming, including decoded-body limits; compressed responses are refused initially. Paging tools default to 100 records and allow at most 500 per page. Endpoints without server pagination can be paged locally only after a complete bounded response is obtained. Exceeded limits fail explicitly, never become an empty inventory or an unlabeled partial result.

## Household coverage and tool policy

Maintain one reviewed API coverage inventory based on Grocy's upstream specification and verify it against the Docker-tested version. Each household operation maps to an implemented tool, an API-defined read-only entity, or an explicit version/capability limitation. A tool count alone is not evidence of full coverage.

| Domain | Required capabilities |
| --- | --- |
| Stock and products | Product CRUD/copy when supported, barcodes, locations, groups, units/conversions, details, entries, volatile stock, price/history views, add/consume/open/transfer/inventory, supported entry edits and transaction undo |
| Shopping | List and item CRUD, quantities/notes, list clearing, missing-products and recipe-derived additions |
| Recipes and planning | Recipe CRUD, ingredients/nesting, fulfillment, consume/undo, meal plans and sections |
| Chores, tasks, batteries | Record CRUD, execution/completion/charge actions, rescheduling and supported undo/history reads |
| Equipment and custom data | Equipment CRUD, supported user fields/entities/objects and their values |
| Files and exports | API-addressed image/manual/user-file read/upload/delete, iCalendar export, and supported shopping-list export |

Allow only reviewed household entities and documented methods. Do not directly edit Grocy's read-only stock/history views. Restrict user lookup to assignment identifiers and names needed for household operations; exclude password/account fields, sessions, API keys, permissions, server configuration secrets, and user/account mutation. Public sharing-link operations are excluded. Custom data metadata is handled through the documented household API, not an unrestricted developer passthrough.

File tools accept an API file group, a filename, and bounded encoded content for uploads; they do not read/write host paths. Encode Grocy filenames as required by the API and send actual binary upload bodies. Return validated images as MCP image content and other files as bounded embedded resources with an explanatory text block. No implicit fetching of file links. API files can be addressed from known names/references; do not claim access to orphaned files or an API file-listing capability Grocy does not provide.

All household writes are enabled for the user's chosen full-access profile. An optional read-only profile omits mutation tools. Local policy still excludes administration and arbitrary requests. Destructive tools carry honest annotations and identify the target precisely; no universal approval token or per-write terminal prompt is added. The host's approval policy remains available independently.

## Mutation correctness and responses

Resolve identifiers before mutations. Ambiguous names return candidates instead of choosing one. Treat stock quantities as the product's stock unit unless an explicit supported conversion is requested; never infer a unit from a product name or silently treat grams as kilograms. Validate positive amounts, dates, locations, required product metadata, and mutation-specific constraints. Product stock-unit changes require a valid conversion and Grocy's supported procedure. Use decimal-aware validation to avoid avoidable floating-point errors.

Never automatically replay a write after a timeout, cancellation, or transport failure. Once dispatch may have occurred, return `UNKNOWN_WRITE_OUTCOME`, the operation and nonsensitive target identifiers, and instructions to inspect current state. Return transaction/object IDs when the API provides them and perform one suitable readback for state-changing operations where available. Preserve valid empty/204 success responses honestly. Verification failure after a successful write is reported as a completed write with incomplete verification, not as a failed write inviting a repeat.

Grocy's documented API does not provide general calendar-style revision checks. Serialize writes within one MCP process and use preflight/readback checks where useful, while documenting that these do not prevent concurrent changes by other clients. Do not claim atomic conditional edits or implement a fictitious revision guarantee.

Every data-returning tool includes actual data in JSON text and structured content, including valid grouped volatile-stock responses. Pagination always identifies whether more data exists. Errors set `isError` and provide a stable code and useful redacted message. Do not forward raw upstream errors, credentials, URLs with secrets, headers, traces, or private profile contents. Descriptions and schemas contain generic examples, units, date rules, paging, deletion behavior, and uncertain-write recovery instructions; no reliance on previous chat memory.

## Credential storage and guided setup

Store the full private profile as one Secret Service item: API base, key, access mode, timezone/defaults discovered from Grocy, optional CA path, and explicit enrollment choices. Keyring attributes contain only the application identifier and nonsecret local profile name. Restricted atomic preference files contain only provider selection/identity; generated Hermes entries contain only executable, profile and provider arguments.

Discover available/running providers without reading unrelated secrets, creating collections or activating inactive services. Recommend the backend appropriate to the desktop; reuse an existing selection and ask the user when multiple providers are available. Bind each secret operation to the selected provider's unique D-Bus owner, with no silent provider fallback. Use encrypted Secret Service IPC through maintained OS/library interfaces; do not design a new cryptographic scheme or silently downgrade to plaintext sessions. Runtime does not unlock/create wallets.

Use the calendar project's conservative protection model: recognize bounded owned-file header evidence where reliable, distinguish unprotected from unverified storage, and explain native-manager steps. KWallet format does not prove a nonempty password. Offer understandable acknowledgement after the user protects an unverified wallet, plus an explicit default-off unsafe override clearly repeated in final review. Known unprotected storage can persist secrets in plaintext; label that risk honestly. No application credential fallback to files, environment variables, CLI arguments, or MCP inputs is introduced, even when the user accepts a backend-risk override.

Credentials necessarily exist in memory during use. Zeroize application-owned secret buffers where practical, while avoiding a claim that TLS/library copies, swap, core dumps or same-user/privileged inspection are eliminated. The MCP does not persist household data, but Hermes history and its model provider can retain returned content; describe that separate privacy boundary in setup/security guidance. Returned household text is untrusted data and cannot change configuration or trigger external fetches.

Setup asks only for the instance/API URL and a hidden API key when needed. Explain creating a dedicated Grocy user, selecting household permissions, and using the UI's Manage API keys page. Explain reviewing any upstream identity-provider 2FA policy; do not imply the MCP automatically configures Authelia or server accounts. Read `/api/system/info` and safe household metadata to validate connectivity and discover available features, units/locations and usable defaults. Reuse existing profile choices during re-enrollment. Present one final review before atomic replacement.

Offer native-manager help and retries for locked/unavailable wallets, and actionable recovery for TLS/API/proxy problems without starting setup over. Use the installed Hermes preserving configuration writer for a single credential-free `grocy` entry, refuse conflicts, and verify discovery without household mutations. If unavailable, print a manual block and recovery instructions. `forget` removes only this application's selected local profile; revoking the server key remains a clearly explained user action.

## Verification, efficiency, and delivery

Use a compact set of parameterized boundary tests: API-base/route construction and traversal rejection; redirect/proxy/TLS behavior; bounded responses; missing/locked/unavailable/encrypted-IPC credential cases; response content/annotations; quantity validation; and uncertain-write no-replay behavior. Add tests for materially different behavior, not one nearly identical test per route.

Run one reusable integration scenario against a digest-recorded LinuxServer.io Grocy Docker image with synthetic data, a dedicated disposable container/network, loopback exposure, and disposable state. Bootstrap the fixture independently of the MCP; fixture administration is not a production MCP capability. Put an API-only proxy in front of it that rejects non-API paths and assert all MCP setup/doctor/workflow requests stay within the configured prefix. Exercise actual MCP discovery/calls and representative CRUD/action/undo workflows across every household domain, including a file round trip and prefixed deployment. Use synthetic provider injection confined to the test harness, with no shipped plaintext credential option.

The public demo is read-only reference material. Its HTML documentation and successful API version response have been checked; an empty/non-JSON `/api/openapi.json` response must not be mistaken for a usable schema. Use the upstream repository specification as the development reference when the demo does not supply one.

Run formatting, compiler/lint checks, the focused test suite, the Docker scenario, and one dependency/privacy review before delivery. Repeat checks only after relevant changes or failures. Perform one final review against this spec, fix concrete findings, and verify affected checks. No arbitrary test-count target or repeated broad review rounds.

Keep container state, credentials, captures, local reports and build output outside tracked source. Remove only the disposable test resources created for this project after verification; do not prune other Docker resources. Record the verified Grocy version/image digest and practical limits in public docs without local identifiers. Ship README/setup/security guidance, a coverage table, lockfile, purposeful tests and AGPL-3.0-only licensing. Publication and real-instance enrollment happen after the implementation is concrete and verified, following the established privacy and contributor-identity requirements.

## Primary references

- [Official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)
- [Grocy API specification](https://github.com/grocy/grocy/blob/master/grocy.openapi.json)
- [Public API reference](https://demo.grocy.info/api)
- [Maintained Grocy Docker image](https://docs.linuxserver.io/images/docker-grocy/)

## Acceptance

The approved full-access scope works through `/api` behind a frontend-blocking proxy; Hermes receives real data; credentials use the explicitly selected wallet without application plaintext persistence; uncertain writes are not replayed; supported household domains and binary files work in the disposable integration fixture; setup and recovery are understandable without an LLM; the repository contains only public-facing source/docs/tests and uses the contributor's public identity without assistant attribution.
