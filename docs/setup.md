# Guided setup

Build/install the executable using the README, then run:

```sh
hermes-grocy-mcp setup
```

You normally enter only your Grocy address and a hidden API key. Arrow keys select a menu item; Enter accepts a suggested choice. Setup discovers your desktop wallets, suggests the appropriate provider, checks the API, discovers household metadata and your desktop time zone, and shows a final review before saving. It never asks you to search for an IANA timezone.

## Prepare your Grocy account

1. Ask your Grocy administrator to create a dedicated Hermes user in **Manage users**. Give it only the household permissions you want the assistant to use. Full household access can create, edit and delete records; custom field/entity definitions may require broader permissions.
2. Sign in as that user. In its user menu, open **Manage API keys**, add a key, and copy it into setup's hidden prompt. Use this key rather than an account password. Keys inherit that user's server permissions.
3. Strongly recommend enabling 2FA for the identity provider protecting your Grocy login, such as Authelia. An API key can bypass frontend 2FA: protect it independently, and revoke it in **Manage API keys** if compromised.

Enter your normal HTTPS Grocy homepage, or its API address. A deployment prefix is preserved: `https://grocy.example/household` becomes `https://grocy.example/household/api`. Do not enter a UI page such as `/stockoverview`. All MCP requests use the API; an Authelia deployment needs to permit the enrolled `/api` prefix with Grocy API-key authentication. Complete Grocy's initial migration using its UI separately.

## Your wallet

Setup detects active and activatable Secret Service providers. With several providers it offers a choice; on KDE it recommends KWallet, on GNOME GNOME Keyring. It never changes providers silently or reads unrelated secrets. Open/unlock the selected default wallet in its own manager. Installing a manager is unnecessary if your wallet is already configured.

- **KDE:** open KWallet Manager and the default wallet, often `kdewallet`. Use **File → Change Password** to set a non-empty password. Keep the wallet enabled. KWallet's file format cannot prove that its password is non-empty, so setup explains how to check and asks you to acknowledge that step.
- **GNOME:** open **Passwords and Keys** (Seahorse), select the default keyring and use **Change Password**. Set a non-empty password. Setup checks the available file header evidence; this is evidence about storage format, not a complete audit of the wallet.
- **Other providers:** protect/unlock their default collection using their native manager. Unsupported encryption is refused.

If protection cannot be confirmed, the menu offers help, acknowledgment of a password you set, cancellation, or an explicit unsafe-storage override (default off). The override may permit plaintext *wallet storage*. It **never** permits plaintext application-to-wallet communication: the encrypted Secret Service session remains mandatory. The normal path stores the entire private profile, including address, key and household metadata, in your selected wallet.

Setup confirms the saved profile by reading it back through a fresh encrypted wallet session before reporting success. Wallet/network recovery keeps answers in memory for retries. Re-running setup reuses the selected provider, saved address/key and access mode, then asks before replacing the profile. A read-only enrollment is available with `setup --read-only`. To enable writes on a previously read-only profile, forget that profile and enroll it again, or use a separate profile (`setup --profile household`).

## Hermes connection

Setup uses the installed `hermes config` preserving writer to add `mcp_servers.grocy`, refuses a conflicting entry and runs `hermes mcp test grocy` (discovery only). Other settings are preserved. Run `/reload-mcp` in Hermes, or start a new session. Try:

> Use grocy to list my stock. Do not change anything.

If Hermes is unavailable or its interface differs, enrollment still completes. Put the installed `hermes` executable on your PATH and run `hermes-grocy-mcp connect-hermes`. Alternatively run `hermes-grocy-mcp print-config` and merge the printed `grocy` entry into `mcp_servers` in the file shown by `hermes config path`. The JSON entry is also valid YAML. It contains only executable, profile and provider identifiers—no key or server address. Keep the executable at the registered path.

## Troubleshooting and removal

`hermes-grocy-mcp doctor` checks the selected wallet and household API without writes. Unlock the same wallet before starting Hermes, including when using a system service; a service needs access to your user's desktop D-Bus session. Headless credential storage is not provided.

Setup explains connection recovery: check the address, API bypass and key permissions; replace the key or select a trusted PEM CA file for a private certificate authority. There is no insecure TLS option. `--allow-loopback-http` is explicitly for local development and only accepts a literal loopback address, not a LAN host.

`hermes-grocy-mcp forget` confirms removal of this application's matching wallet item and preferences. It does not revoke the server key or remove the Hermes entry. Revoke the key in Grocy and remove `mcp_servers.grocy` separately if you no longer use it.
