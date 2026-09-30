# Hermes Grocy MCP

A Linux desktop stdio MCP server written in Rust. Connect Hermes to your self-hosted Grocy using its API, with the private profile in your selected desktop wallet. Inventory and other reads return actual JSON in both text and structured MCP results. Writes return the upstream receipt and a bounded readback where available.

| Household area | Capabilities |
| --- | --- |
| Stock | Products, barcodes, locations, groups, units/conversions; add, consume, open, transfer, inventory, entry editing, copy/merge, history and supported undo |
| Shopping | Lists, items, quantities, notes, stores, missing/due-product additions, clearing and JSON export |
| Recipes and meals | Recipes, ingredients, nesting, fulfillment, consumption, copies, meal plans and sections |
| Household work | Chores and executions, tasks and completion, batteries and charge cycles; supported undo and record edits |
| Equipment and custom data | Equipment, household custom fields/entities/objects and their values |
| Files and dates | Known household API files, bounded binary upload/download/delete, iCalendar export |

Full access includes deletion. `setup --read-only` omits mutation tools and enforces read-only access again at the HTTP boundary. Account administration, API keys, sessions, configuration secrets, external lookups, sharing and printer endpoints are excluded. See [coverage](docs/api-coverage.md) and [tested compatibility](docs/compatibility.md).

## Get started

Requires a current stable Rust toolchain, a Linux desktop session with a compatible Secret Service wallet, and libsecret/GLib. KWallet and GNOME Keyring use their existing default wallet; setup recommends the provider for your desktop. A separate wallet-manager GUI is only needed to change/unlock its password.

Build prerequisites (choose your distribution):

```sh
# Arch Linux
sudo pacman -S --needed base-devel pkgconf libsecret glib2
# Debian / Ubuntu
sudo apt install build-essential pkg-config libsecret-1-dev libglib2.0-dev
# Fedora
sudo dnf install gcc pkgconf-pkg-config libsecret-devel glib2-devel
```

In this repository:

```sh
cargo install --path . --locked
hermes-grocy-mcp setup
```

Setup explains how to create a dedicated Grocy user and API key, asks for an address and hidden key, guides wallet protection, checks only the API, and adds a credential-free Hermes entry using its preserving configuration writer. Unlock the selected wallet before starting Hermes. In Hermes, run `/reload-mcp` or start a new session, then try:

> Use grocy to list my stock. Do not change anything.

Follow the [setup guide](docs/setup.md) for account permissions, 2FA, Authelia, custom certificates, retries and manual registration. Do not put your API key in environment variables, command arguments, YAML or a `.env` file. None of those credential-loading mechanisms are supported.

## Security and privacy

All requests remain under the enrolled HTTPS `/api` prefix, including setup, files and exports. Deployment prefixes work. Redirects, ambient proxies, automatic request retries and compressed responses are disabled; TLS verification stays enabled. Input/response limits are enforced. MCP tools cannot supply a destination, header or host filesystem path.

The full private profile is stored as one wallet item. Only the profile label/provider identity are kept in restricted local preferences; the Hermes entry contains no key or server address. Application-to-wallet communication requires the encrypted Secret Service session. Setup distinguishes detected, unverified and unsafe storage; its explicit unsafe override can permit an unprotected wallet, never plaintext IPC.

A write with `UNKNOWN_WRITE_OUTCOME` may already have happened. Inspect current state and history before another write; do not retry it automatically. A completed write with `verification: incomplete` also must not be repeated just because its readback failed. Writes are serialized within one process; Grocy does not provide general conditional revisions across clients.

Returned household data can be retained by Hermes or its model provider. Read the [security policy and limits](SECURITY.md) before enrolling a real key.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests additionally need `openssl`, `dbus-daemon` and Python 3. The focused suite covers destination/TLS limits, isolated wallet policy, catalog validation, MCP data, write outcomes, files and setup. The Docker contract is explicit and uses only disposable synthetic data:

```sh
python3 scripts/docker_fixture.py start --state /tmp/grocy-mcp-contract
python3 scripts/docker_fixture.py test --state /tmp/grocy-mcp-contract
python3 scripts/docker_fixture.py stop --state /tmp/grocy-mcp-contract
```

Choose a fresh state directory outside the repository. Run `stop` even when a test fails. The runner removes only its labeled container, network and volume; it never prunes unrelated resources. Fixture bootstrap may visit the disposable UI for migrations. Every MCP call is tested behind a proxy that permits only root/prefixed API paths.

AGPL-3.0-only. See [LICENSE](LICENSE). Third-party dependencies retain their own licenses.
