# Security policy and limits

This project handles a Grocy API key and household data. Report a suspected vulnerability privately to the repository owner's security contact or private vulnerability-reporting facility when available; do not include a live key or household export in a public issue. Revoke an exposed key in Grocy immediately. There is no claim of a professional or independent security audit.

## Boundaries

- One enrolled HTTPS origin and deployment-aware `/api` prefix. Only finite reviewed routes are exposed. No arbitrary HTTP requests, redirects, frontend scraping, external product lookups or MCP filesystem access.
- Verified TLS, optional locally selected PEM CA, no insecure TLS flag. Ambient proxy variables are ignored. Explicit literal loopback HTTP is a development enrollment option only.
- No server listener: local stdio MCP. Raw input frames are bounded to 12 MiB; ordinary tool arguments/JSON/text responses to 2 MiB; decoded API files to 8 MiB. File groups/names are constrained and encoded as required by Grocy. Compressed API responses are refused.
- The selected desktop provider's executable identity is checked on each secret operation, then the operation binds to its unique D-Bus owner. A restart/change cannot redirect an in-flight operation to another owner. No silent provider fallback, wallet creation/unlock or unrelated secret reads.
- Maintained libsecret performs the encrypted Secret Service exchange. Its encrypted session algorithm is checked before accessing a profile. The protocol's standardized 1024-bit DH session is not a claim of modern transport strength against privileged local attackers; this is a desktop IPC boundary, not an Internet transport.
- The private profile (address, key, metadata and options) is one wallet secret. Preferences are atomically written with mode 0600 in an owned mode-0700 directory and contain only provider identity/profile label. Generated Hermes entries contain no credentials or address. Enrollment does not accept credentials from arguments, environment or files.
- Wallet storage evidence is conservative. GNOME file-header evidence is not a complete protection audit. KWallet cannot prove a non-empty password through this mechanism; setup gives native steps and requires an acknowledgment or a default-off unsafe override. **The override may persist plaintext inside the wallet. It never relaxes encrypted IPC.**
- Read-only policy is checked in tool discovery, validation and HTTP dispatch. Full access deliberately permits household deletion and undo; server user permissions still apply. Definitions and values of account custom fields are not exposed.

## Mutation semantics

Preflight checks validate record existence, explicit IDs and unit conversions. Ambiguous names return candidates. Quantities are converted with checked decimal arithmetic. Changing a stock unit uses Grocy's database cascade only after a positive unambiguous conversion exists. Products must have the same stock unit before merging; a record cannot merge with itself.

Grocy has no general ETag/conditional revision guarantee. A mutex serializes mutations in one MCP process, but another process, browser or API client can change state between a preflight and write. Stock-entry edits preserve omitted fields from a preflight read and have this same race limitation. `verification: confirmed` means the readback was obtained (or file bytes/deletion absence confirmed); it is not a cross-client atomicity guarantee.

The HTTP client never retries requests. A connection interruption, server failure or cancellation after possible dispatch yields `UNKNOWN_WRITE_OUTCOME`, with target identifiers for inspection. Do not repeat automatically. Once the server returns a valid success, a later readback failure is `completed` with `verification: incomplete`. Empty successful receipts remain empty; recipe consumption's undo identifiers must be found in stock history, with care around concurrent activity.

Grocy can run its own configured automation, constraints, cascades and hooks when household data changes. Excluding printer endpoints does not disable server-side policies configured by the Grocy administrator. Review those policies and the dedicated user's permissions.

## Privacy and host assumptions

The MCP does not log requests, raw upstream errors, keys, private profile contents or household responses. Errors use fixed public messages. It does not maintain a household database/cache or telemetry. The wallet's public item attributes include an application identifier and your nonsecret profile label. The local Hermes command/path and provider identity remain visible to your user.

Secrets necessarily enter process memory. Application-owned key and serialization buffers are zeroized where practical; TLS, JSON, GLib/library allocations, OS memory, swap and core dumps are not comprehensively erased. This does not protect against root, a compromised desktop user, a malicious selected wallet, an untrusted Hermes executable, malicious dependencies or a compromised Grocy server. Keep the host and wallet updated, use disk/swap protection and restrict debugging/core dumps according to your needs.

Household text is untrusted data, not an instruction to change configuration or fetch external links. Grocy file bytes are returned as bounded MCP images/resources, with image signature checks; they are not executed or written to host paths by the server. The consuming client may render/store them. Hermes history and remote model providers may retain all returned household content: their data policies are separate from this MCP's storage policy.

The shipped credential backend targets Linux desktop Secret Service. There is no headless plaintext fallback or Windows/macOS backend. Dependency advisories are a point-in-time check; keep reviewing updates and the lockfile. Compatibility and actual verification evidence are documented in [docs/compatibility.md](docs/compatibility.md).
