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

`setup --read-only` omits mutation tools and enforces read-only access again at the HTTP boundary. Account administration, API keys, sessions, configuration secrets, external lookups, sharing and printer endpoints are excluded. See [coverage](docs/api-coverage.md) and [tested compatibility](docs/compatibility.md).

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

## Tools

`serve` offers one of two tiers:

| Flag | Effect |
| --- | --- |
| `--tools lite` (default) | 17 task-shaped tools that accept names. Suited to small local models. |
| `--tools full` | Every reviewed Grocy operation (ID-based) plus the lite tools. |
| `--allow-destructive` | Adds deletes, merges and list clearing, plus edits to units and custom fields/entities (full tier only). Each destructive call asks the human through MCP elicitation. If the client cannot ask, the call is refused. |

The lite tools are `find_product`, `whats_low_or_expiring`, `add_stock`, `consume_stock`, `open_product`, `inventory_set`, `create_product`, `shopping_list`, `shopping_add`, `shopping_remove`, `recipes_suggest`, `todo_list`, `mark_done`, `meal_plan`, `meal_plan_add`, `undo` and `doctor`. To change the tier, add the flag to the `serve` arguments of the `grocy` entry in Hermes.

Products, units, locations, lists, chores, tasks, recipes and meal-plan sections can be given as an id, a product barcode, a case-insensitive exact name or a unique part of a name. Anything ambiguous or unknown is refused, with up to 8 candidates (`id`, `name`, `unit`, `stock`). Products are never created implicitly: `add_stock` with an unknown name suggests `create_product`. Amounts use the product's stock unit unless `unit` names another unit with a Grocy conversion. Stock results include the new amount, the unit name and a `transaction_id`.

`undo` reverts the most recent stock, chore, task or battery write made through this server process (it keeps the last 20), or one given `transaction_id`. Grocy merges identical purchases into a single stock entry, and undoing one of them would delete the whole entry, so such undos are refused with a suggested correction.

Errors name the argument at fault (for example `amount: must be greater than 0`). Grocy's own message is included, truncated to 300 characters with a hint, never with stack traces, URLs or secrets. Results above 16 KiB come with a short text summary; the complete JSON is always in `structuredContent`. Tool annotations mark read-only and destructive tools.

`hermes-grocy-mcp doctor [--profile …]` (and the `doctor` tool) prints pass/warn/fail checks for the wallet, profile, TLS, API reachability, key, Grocy version, access mode and feature flags. It never prints the key or changes anything.

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

Tests additionally need `openssl`, `dbus-daemon`, `dbus-run-session`, `gnome-keyring-daemon` and Python 3. The encrypted wallet regression uses a private bus and temporary keyring; it does not access your desktop wallet. The focused suite covers destination/TLS limits, isolated wallet policy, catalog validation, MCP data, write outcomes, files and setup. The Docker contract is explicit and uses only disposable synthetic data:

```sh
python3 scripts/docker_fixture.py start --state /tmp/grocy-mcp-contract
python3 scripts/docker_fixture.py test --state /tmp/grocy-mcp-contract
python3 scripts/docker_fixture.py stop --state /tmp/grocy-mcp-contract
```

Choose a fresh state directory outside the repository. Run `stop` even when a test fails. The runner removes only its labeled container, network and volume; it never prunes unrelated resources. Fixture bootstrap may visit the disposable UI for migrations. Every MCP call is tested behind a proxy that permits only root/prefixed API paths.

AGPL-3.0-only. See [LICENSE](LICENSE). Third-party dependencies retain their own licenses.
