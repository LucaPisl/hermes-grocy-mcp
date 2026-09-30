# API coverage

Catalog baseline: Grocy 4.7.1. All remote operations use the enrolled `/api` prefix, including files and diagnostics. Record fields are a reviewed factual catalog derived from that version's schema; new server fields require a catalog update.

Household records cover products/barcodes/groups, locations, units/conversions, shopping lists/items/stores, recipes/ingredients/nesting, meal plans/sections, chores, tasks/categories, batteries, equipment and custom fields/entities/objects. Supported history and stock views are read-only.

Account mutation, API keys, sessions, permissions, configuration secrets, public sharing links, external lookup and printing are excluded. Stock-unit changes require a positive unambiguous Grocy conversion. Grocy 4.7.1's database cascade rescales stock and related records; its API result is read back. These preflight checks do not prevent simultaneous edits by other clients.

Source: [Grocy API specification](https://github.com/grocy/grocy/blob/master/grocy.openapi.json).
