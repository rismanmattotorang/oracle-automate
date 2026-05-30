---
name: oracle.skill.po_creation_elicit
description: Guided purchase-order creation with mid-execution elicitation for charge account, business unit, and delivery date.
tags: [procurement, purchase-order, elicitation, workflow]
requires_tools: [oracle.workflow.create_purchase_order, oracle.rest.metadata, oracle.docs.search]
arguments:
  - name: supplier_hint
    description: Supplier hint, e.g. "PT Sumber Daya Komputasi"
    required: false
  - name: item_hint
    description: Item number hint, e.g. "GT-COMP-GPU-A100"
    required: false
---

Create a purchase order for **{{supplier_hint}}** / **{{item_hint}}** using the guided workflow.

The `oracle.workflow.create_purchase_order` tool pauses mid-execution and asks the user to confirm:

- Supplier and supplier site
- Item, quantity, and UOM
- **Charge account / cost centre** — high-stakes; never inferred silently
- Business unit and currency
- Requested delivery date

Steps:

1. Optionally run `oracle.docs.search` with `"create purchase order REST purchaseOrders"` to confirm the procedure.
2. Optionally run `oracle.rest.metadata` for `fusion.po.purchaseOrders.post` to confirm the parameter shape that will fire downstream.
3. Call `oracle.workflow.create_purchase_order` with any supplier/item hints you have. The tool pauses and asks the user to confirm the form. The user can accept, decline (cancels without side-effects), or cancel.
4. If accepted and the server was started with `--enable-writes`, the next step calls `oracle.rest.call` for `fusion.po.purchaseOrders.post`. Do NOT proceed without the user's explicit confirmation in the elicitation form.

Cite the operation URI (`oracle-rest://fusion.po.purchaseOrders.post`) and the procedure page in the final summary.
