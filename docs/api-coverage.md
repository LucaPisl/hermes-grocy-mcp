# API coverage

Catalog baseline: Grocy 4.7.1. All remote operations use the enrolled `/api` prefix, including files and diagnostics. Record fields are a reviewed factual catalog derived from that version's schema; new server fields require a catalog update.

Household records cover products/barcodes/groups, locations, units/conversions, shopping lists/items/stores, recipes/ingredients/nesting, meal plans/sections, chores, tasks/categories, batteries, equipment and custom fields/entities/objects. Supported history and stock views are read-only.

Account mutation, API keys, sessions, permissions, configuration secrets, public sharing links, external lookup and printing are excluded. Stock-unit changes require a positive unambiguous Grocy conversion. Grocy 4.7.1's database cascade rescales stock and related records; its API result is read back. These preflight checks do not prevent simultaneous edits by other clients.

Source: [Grocy API specification](https://github.com/grocy/grocy/blob/master/grocy.openapi.json).

## Household entity inventory

Use `records_describe` for required creation fields, then `records_list`/`records_lookup` to find explicit IDs. `records_create`, `records_update` and `records_delete` are limited to editable entities. `userfields_get`/`userfields_update` read/change defined household values. For a custom object use `entity: userobjects` and its object ID; the MCP resolves its custom entity from the stored definition. Define its fields against `userentity-<name>` after creating that household entity. Account custom-field definitions are filtered from reads and cannot be mutated. For stock batch values, pass the numeric entry row ID plus an exact `stock_transaction_id` from a purchase/inventory-correction/entry-edit receipt or history. Grocy inserts these fields via transactions; reads resolve the batch ID automatically. A transaction spanning other batch IDs is refused.

| Entity | Access |
| --- | --- |
| `products` | Read/create/update/delete |
| `chores` | Read/create/update/delete |
| `product_barcodes` | Read/create/update/delete |
| `batteries` | Read/create/update/delete |
| `locations` | Read/create/update/delete |
| `quantity_units` | Read/create/update/delete |
| `quantity_unit_conversions` | Read/create/update/delete |
| `shopping_list` | Read/create/update/delete |
| `shopping_lists` | Read/create/update/delete |
| `shopping_locations` | Read/create/update/delete |
| `recipes` | Read/create/update/delete |
| `recipes_pos` | Read/create/update/delete |
| `recipes_nestings` | Read/create/update/delete |
| `tasks` | Read/create/update/delete |
| `task_categories` | Read/create/update/delete |
| `product_groups` | Read/create/update/delete |
| `equipment` | Read/create/update/delete |
| `userfields` | Read/create/update/delete |
| `userentities` | Read/create/update/delete |
| `userobjects` | Read/create/update/delete |
| `meal_plan` | Read/create/update/delete |
| `stock_log` | Read only |
| `stock` | Read only |
| `stock_current_locations` | Read only |
| `chores_log` | Read only |
| `meal_plan_sections` | Read/create/update/delete |
| `products_last_purchased` | Read only |
| `products_average_price` | Read only |
| `quantity_unit_conversions_resolved` | Read only |
| `recipes_pos_resolved` | Read only |
| `battery_charge_cycles` | Read only |
| `product_barcodes_view` | Read only |
| `uihelper_shopping_list` | Read only |

## Named upstream operations

All routes below are relative to the enrolled API base. Local descriptions/validation and credential loading do not make HTTP requests. Generic record tools use `/objects/{entity}` or `/objects/{entity}/{id}`; custom values use `/userfields/{entity}/{id}`. An unavailable server route/record returns `NOT_FOUND`, rejected inputs/permissions return sanitized errors, and an interrupted mutation can return `UNKNOWN_WRITE_OUTCOME`.

| Tool | Method | API route |
| --- | --- | --- |
| `database_changed_time` | GET | `/system/db-changed-time` |
| `server_time` | GET | `/system/time` |
| `stock_list` | GET | `/stock` |
| `stock_entry` | GET | `/stock/entry/{entryId}` |
| `stock_entry_put` | PUT | `/stock/entry/{entryId}` |
| `stock_volatile` | GET | `/stock/volatile` |
| `stock_product` | GET | `/stock/products/{productId}` |
| `stock_locations` | GET | `/stock/products/{productId}/locations` |
| `stock_entries` | GET | `/stock/products/{productId}/entries` |
| `stock_price_history` | GET | `/stock/products/{productId}/price-history` |
| `stock_add` | POST | `/stock/products/{productId}/add` |
| `stock_consume` | POST | `/stock/products/{productId}/consume` |
| `stock_transfer` | POST | `/stock/products/{productId}/transfer` |
| `stock_inventory` | POST | `/stock/products/{productId}/inventory` |
| `stock_open` | POST | `/stock/products/{productId}/open` |
| `stock_copy` | POST | `/stock/products/{productId}/copy` |
| `stock_merge` | POST | `/stock/products/{productIdToKeep}/merge/{productIdToRemove}` |
| `stock_product_by_barcode` | GET | `/stock/products/by-barcode/{barcode}` |
| `stock_add_by_barcode` | POST | `/stock/products/by-barcode/{barcode}/add` |
| `stock_consume_by_barcode` | POST | `/stock/products/by-barcode/{barcode}/consume` |
| `stock_transfer_by_barcode` | POST | `/stock/products/by-barcode/{barcode}/transfer` |
| `stock_inventory_by_barcode` | POST | `/stock/products/by-barcode/{barcode}/inventory` |
| `stock_open_by_barcode` | POST | `/stock/products/by-barcode/{barcode}/open` |
| `stock_location_entries` | GET | `/stock/locations/{locationId}/entries` |
| `shopping_add_missing_products` | POST | `/stock/shoppinglist/add-missing-products` |
| `shopping_add_overdue_products` | POST | `/stock/shoppinglist/add-overdue-products` |
| `shopping_add_expired_products` | POST | `/stock/shoppinglist/add-expired-products` |
| `shopping_clear` | POST | `/stock/shoppinglist/clear` |
| `shopping_add_product` | POST | `/stock/shoppinglist/add-product` |
| `shopping_remove_product` | POST | `/stock/shoppinglist/remove-product` |
| `stock_booking` | GET | `/stock/bookings/{bookingId}` |
| `stock_undo_booking` | POST | `/stock/bookings/{bookingId}/undo` |
| `stock_transaction` | GET | `/stock/transactions/{transactionId}` |
| `stock_undo_transaction` | POST | `/stock/transactions/{transactionId}/undo` |
| `recipe_add_not_fulfilled_products_to_shoppinglist` | POST | `/recipes/{recipeId}/add-not-fulfilled-products-to-shoppinglist` |
| `recipe_fulfillment` | GET | `/recipes/{recipeId}/fulfillment` |
| `recipe_consume` | POST | `/recipes/{recipeId}/consume` |
| `recipes_fulfillment` | GET | `/recipes/fulfillment` |
| `recipe_copy` | POST | `/recipes/{recipeId}/copy` |
| `chores_list` | GET | `/chores` |
| `chore_details` | GET | `/chores/{choreId}` |
| `chore_execute` | POST | `/chores/{choreId}/execute` |
| `chore_undo_execution` | POST | `/chores/executions/{executionId}/undo` |
| `chores_recalculate_assignments` | POST | `/chores/executions/calculate-next-assignments` |
| `chore_merge` | POST | `/chores/{choreIdToKeep}/merge/{choreIdToRemove}` |
| `batteries_list` | GET | `/batteries` |
| `battery_details` | GET | `/batteries/{batteryId}` |
| `battery_charge` | POST | `/batteries/{batteryId}/charge` |
| `battery_undo_charge` | POST | `/batteries/charge-cycles/{chargeCycleId}/undo` |
| `tasks_list` | GET | `/tasks` |
| `task_complete` | POST | `/tasks/{taskId}/complete` |
| `task_undo` | POST | `/tasks/{taskId}/undo` |
| `file_get` / `file_upload` / `file_delete` | GET / PUT / DELETE | `/files/{group}/{base64Filename}` |
| `calendar_export` | GET | `/calendar/ical` |
| `shopping_export` | GET | `/objects/shopping_list` (filter explicit list ID) |
| `assignment_users` | GET | `/users` (return only IDs/display names) |

File groups: `equipmentmanuals`, `recipepictures`, `productpictures`, `userfiles`; avatars are excluded. Stock log/history views remain read-only, while supported booking/transaction undo has named action tools. Recipe-derived shopping additions use Grocy's API default list; the endpoint has no explicit list selector. This server does not invent one.

## Explicit exclusions

No arbitrary API request tool. No system configuration/user-settings secrets, API-key/session/permission administration, user/account mutation, public sharing-link routes, barcode external lookup, printer/webhook endpoints or attachments outside `/api`. Assignment labels are the only account projection. Server-configured hooks can still run as part of Grocy's own household policies.

The catalog covers the reviewed household API surface, not undocumented plugins, server internals, database maintenance, filesystem or orphan-file enumeration. The Docker contract exercises a meaningful scenario per domain; it does not claim every possible parameter combination or server-version behavior has been tested.
