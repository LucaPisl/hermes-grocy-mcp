# Compatibility and verification

Baseline: **Grocy 4.7.1**, Linux desktop, official Rust MCP SDK (`rmcp` 1.8.0 resolved in Cargo.lock). Household field definitions were reviewed from that release's database schema and API specification because its OpenAPI omits several schemas and mislabels some numeric path IDs. No assumption is made that future server fields are safely editable: update the catalog deliberately.

The disposable contract pins the LinuxServer image:

```
lscr.io/linuxserver/grocy@sha256:3c1ce96cb2d0f0c7ea0c2e0b89f5769be7f2f102204fbeef87aeef56afd6036e
```

The scenario uses actual MCP SDK discovery/calls and synthetic credentials supplied only by test code. A frontend-blocking proxy accepts `/api/…` and `/household/api/…`; setup/doctor and workflows made zero requests outside those prefixes. It covers product/unit/location creation and updates, real inventory, stock-entry editing, opening/transfer/consumption/transaction undo, inventory, conversion and stock-unit cascade, shopping add/remove/clear/export, recipe ingredients/nesting/copy/consumption/undo, meal plans/sections, chore execution/undo, task completion/undo/rescheduling, battery charging/undo, equipment/custom definitions/custom-entity and stock-batch values, account-definition read/write exclusion, a PDF upload/readback/download/delete, iCalendar export, record deletion and read-only rejection.

The small default suite separately checks malformed/traversing destinations, real self-signed TLS rejection, redirects, compression/size caps, interrupted write without replay, plaintext Secret Service refusal against isolated D-Bus, provider identity mismatch, preference ownership/modes/symlinks, field/quantity/name validation, required conversion preflight, MCP result equivalence/annotations, grouped results, successful writes with failed readback, file bytes and setup cancellation/recovery/configuration preservation. A separate private D-Bus session and temporary GNOME keyring exercise real libsecret encrypted profile save, fresh-session readback, replacement and removal, with GLib critical errors treated as failures. Synthetic tests do not claim to have enrolled a real desktop wallet or household account.

Final delivery checks use formatting, Clippy with warnings denied, the focused tests, the explicit Docker contract and a release build. The one-time RustSec check against the current advisory database found no reported vulnerable locked dependency (290 dependencies, 2026-10-01). This is a time-specific advisory result, not proof of absence of vulnerabilities.

## Practical limits

- Other Grocy versions may reject individual routes/fields or need migration. Errors are explicit; no frontend fallback or fabricated writes are performed. Broad server permissions may be required for custom-field/entity administration even when ordinary household actions are allowed.
- Readback confirms a retrieved state, not an atomic conditional revision. Grocy can compact stock entries and change their IDs; an entry readback can become unavailable after a successful edit.
- Recipe consumption returns HTTP 204 and can consume only available ingredients. The MCP reports the empty receipt honestly. Read stock-log records to identify the precise transaction before undo; do not guess during concurrent writes.
- Files require their known group/filename (from records/custom fields). Grocy has no API to enumerate orphan files. The MCP has no host-path or remote-URL import interface.
- Unpaginated upstream endpoints must fit the 2 MiB complete-response bound before local paging. Very large households may need future server-side filtering support. Generic list tools use server pagination where supported; their metadata indicates whether another page exists.
- The private profile contains enrollment metadata samples (up to 100 IDs/names for locations/units/lists, with bounded name lengths); these are setup defaults, not an authoritative live cache. Tool calls read live data.
- Encrypted IPC and provider binding have been tested negatively with an isolated incompatible service. KWallet storage-password strength remains a user/native-manager check. See the setup and security documents.

Primary references: [Grocy API](https://github.com/grocy/grocy/blob/master/grocy.openapi.json), [Grocy source](https://github.com/grocy/grocy), [LinuxServer image documentation](https://docs.linuxserver.io/images/docker-grocy/), [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk), [Secret Service specification](https://specifications.freedesktop.org/secret-service-spec/latest/).
