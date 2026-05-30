---
name: oracle.skill.rest_service_design
description: Design discipline for exposing an Oracle Fusion REST resource (or a custom OIC-fronted service) as an MCP tool surface — metadata-first / attribute-mapping / auth-binding decisions in one place.
tags: [rest, design, proxy, integration]
requires_tools: [oracle.docs.search, oracle.system.info]
arguments:
  - name: service_name
    description: Fusion REST resource name (e.g. "purchaseOrders", "journalEntries") or custom service path
    required: true
  - name: connection
    description: OIC connection name OR direct base URL (e.g. "GT_FUSION_ERP_REST" or "https://gaussian.fa.ocs.oraclecloud.com")
    required: false
---

# Oracle REST service design — generic proxy pattern

One config-driven foundation can expose any Fusion REST resource as MCP tools without per-service code. This skill is the design checklist that turns a resource name into a stable, agent-friendly tool surface.

**Target resource:** `{{service_name}}`
**Connection:** `{{connection}}`

## Step 1 — Describe first

Always fetch the resource `describe` before tool design:

```
GET <base>/fscmRestApi/resources/11.13.18.05/{{service_name}}/describe
Header: REST-Framework-Version: 9
```

Parse the attribute list, child resources (`links` rel=child), and supported actions. **Do not infer** — child resources are only navigable if `describe` lists them. Cite the resulting URI via `oracle.docs.search`.

## Step 2 — Tool surface design

Map REST operations to MCP tool names following the `<domain>.<verb>.<resource>` convention:

| REST operation | MCP tool name | Read-only? |
|---|---|---|
| `GET /<resource>?q=` | `<domain>.search.<resource>` | yes |
| `GET /<resource>/<id>` | `<domain>.get.<resource>` | yes |
| `GET /<resource>/<id>/child/<rel>` | `<domain>.list.<rel>` | yes |
| `POST /<resource>` | `<domain>.create.<resource>` | **no — gate behind `--enable-writes`** |
| `PATCH /<resource>/<id>` | `<domain>.update.<resource>` | **no** |
| `DELETE /<resource>/<id>` | `<domain>.delete.<resource>` | **no** |
| custom action (`POST .../action/<name>`) | `<domain>.<action_lower>` | **no — actions always write** |

## Step 3 — Schema generation

Generate JSON Schema from the `describe` attribute types: `String(maxLength)`→`{type:string,maxLength}`; amounts/`Number`→carry as strings, never floats; `Date`→`{type:string,format:date}`; child resource→`$ref`. Set `"additionalProperties": false` so agents can't invent fields.

## Step 4 — Auth binding

| Connection kind | Auth scheme | Where credentials live |
|---|---|---|
| Fusion REST (direct) | OAuth2 client-credentials (IDCS/IAM) or Basic | OIC connection / `OicAuth` |
| Via OIC | connection security policy | OIC connection |
| Public test pod | Basic | env / keyring via `LayeredCredentialProvider` |

Never store credentials in the tool schema or logs; audit must use `redact_secret()`.

## Step 5 — Read-only safety posture

Default the tool set to read-only; mark writes with `.with_writes()` (hidden from `tools/list` unless `--enable-writes`). High-stakes resources (journals, suppliers, sandboxes) require the elicitation flow before the write.

## Step 6 — Verify

Verifiable criteria: `describe` returns 200 + valid attributes; `tools/list` emits one tool per operation; `search` returns a non-empty result on the live connection; every write tool is hidden without `--enable-writes`; every write tool fires elicitation before the POST/PATCH.

Produce a markdown design doc with one section per step. Do not start coding until reviewed.
