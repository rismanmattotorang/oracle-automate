---
name: oracle.skill.analytics_migration
description: Analytics-modernisation workflow — inventory reporting objects (OTBI / BI Publisher / BICC), classify migration patterns (lift-and-shift vs redesign), produce a phased plan to Oracle Analytics Cloud / Fusion Analytics Warehouse / Autonomous Data Warehouse.
tags: [analytics, otbi, adw, modernization, planning]
requires_tools: [oracle.docs.search, oracle.object.read, oracle.oic.where_used, kb.global_query]
arguments:
  - name: source_object
    description: Reporting object (OTBI analysis, BIP report/data model, BICC extract, subject area) OR "*" for a portfolio-wide inventory
    required: true
  - name: target_release
    description: Target platform (e.g. "Fusion Analytics Warehouse", "Oracle Analytics Cloud", "Autonomous Data Warehouse")
    required: false
---

# Analytics → modern-platform migration planning

The skill produces a **migration design document**, not a migration execution. All operations are read-only.

**Target object:** `{{source_object}}`
**Target platform:** `{{target_release}}`

## Step 1 — Object inventory

For `{{source_object}} == "*"` (portfolio-wide), enumerate the reporting estate:

| Artefact | Where | Key |
|---|---|---|
| OTBI analyses / dashboards | BI Catalog (`/analytics`) | catalog path |
| BI Publisher reports + data models | `oracle.oic.get_bip_report` / `oracle.oic.get_bip_data_model` | report path |
| BICC extracts (offering/VO) | Manage Extract Schedules | offering, view object |
| Custom subject areas | semantic model | subject area |
| Downstream consumers (OIC, OAC) | `oracle.oic.where_used` | integration / report |

For a single object, fetch its definition plus where-used (`oracle.oic.get_bip_report`, `oracle.oic.where_used`).

## Step 2 — Classification matrix

| Source pattern | Target counterpart | Effort |
|---|---|---|
| OTBI analysis on a delivered subject area | OAC workbook / FAW subject area | Low |
| BIP pixel-perfect report | BIP on OAC (lift) or OAC paginated report | Low–Medium |
| BIP data model with custom SQL | ADW view + OAC dataset | Medium |
| BICC → on-prem DW | FAW data pipelines (delivered) or BICC → ADW | Medium |
| Heavily customised RPD-style semantic layer | **Redesign** in FAW semantic model | **High** |
| Reports embedding PL/SQL / Groovy logic | **Manual rewrite** as views / pipeline transforms | **High** |

## Step 3 — Custom-logic surfacing

Every BIP data model with custom SQL or a BICC extract with derived columns will not run as-is. Enumerate them and classify: simple join → view; currency/UOM conversion → built-in; hard-coded business rule → manual rewrite; external call → OIC integration.

## Step 4 — Citation pass

Call `oracle.docs.search` for the Fusion Analytics Warehouse adoption guide and the OAC migration handbook; cite both URIs. For large estates (>50 objects) also call `kb.global_query query="analytics migration patterns"` for the community-summary roll-up.

## Step 5 — Wave plan

Produce a 3-wave plan (Foundation → active workloads → custom-logic redesign → decommission), each wave with verifiable exit criteria.

## Step 6 — Risk register

End with a risk register. Minimum entries: semantic-layer parity, reporting-performance baseline (capture P95), security model differences (run `oracle.skill.security_sod_audit` before cut-over), hidden side-effects in custom SQL (run `oracle.oic.where_used`).

**No write operations** are performed by this skill — the deliverable is a reviewable design document.
