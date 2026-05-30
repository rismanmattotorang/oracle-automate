---
name: oracle.skill.security_sod_audit
description: Segregation-of-Duties (SoD) audit workflow for Oracle Fusion security — read-only analytical review across users, roles (job/duty/data), privileges, and integration accounts, aligned with Oracle Risk Management Cloud (Advanced Access Controls).
tags: [security, sod, audit, governance]
requires_tools: [oracle.object.read, oracle.rest.metadata, oracle.docs.search, oracle.system.info]
arguments:
  - name: user_or_role
    description: Target user name OR role to audit (e.g. "EDWIN" or "ORA_GL_FINANCIAL_ANALYST_JOB")
    required: true
  - name: scope
    description: Audit scope — "user" (single user) | "role" (single role) | "system" (whole-environment roll-up)
    required: true
---

# Segregation-of-Duties audit — read-only

This skill produces a structured audit report **without writing anything** — no role assignments, no privilege grants, no sandbox/config changes. It mirrors the analytics behind Oracle Risk Management Cloud's Advanced Access Controls (AAC).

**Target:** `{{user_or_role}}` (scope: `{{scope}}`)

## Step 1 — Identify the target

Call `oracle.system.info` first. Record the pod / ledger scope / environment role.

For `{{scope}} == "user"`: read the user record (`PER_USERS` — `USERNAME`, `ACTIVE_FLAG`, `SUSPENDED`, `START_DATE`, `END_DATE`).
For `{{scope}} == "role"`: read the role (`ASE_ROLE_B` / Security Console export — role code, role type job/duty/data, description).

## Step 2 — Enumerate access

- **User → roles:** `PER_USER_ROLES` (`USERNAME`, `ROLE_ID`, `START_DATE`, `END_DATE`).
- **Role → aggregated roles & duties:** role hierarchy (`ASE_ROLE_HIERARCHY` / Security Console).
- **Role → function privileges:** the function-security privileges the role grants.
- **Role → data security policies:** the data scope (`FND_GRANTS` / data-role conditions).

## Step 3 — Apply the SoD rule library

Compare the granted privilege set against canonical Oracle SoD conflict pairs:

| Conflict pair | Risk |
|---|---|
| Create Payables Invoice + Approve Payables Invoice + Submit Payment Process | One person can pay an unreviewed invoice |
| Create Supplier + Create Payables Invoice + Submit Payment | Phantom-supplier fraud |
| Manage Journal + Post Journal + Open/Close GL Period | Unreviewed entries posted into a (re)opened period |
| Create Sales Order + Confirm Shipment + Create AR Invoice | Phantom-customer revenue inflation |
| Manage Users + Manage Role Provisioning + Manage Sandboxes | Self-privilege escalation |

Cite the Oracle SoD / AAC documentation via `oracle.docs.search` for each conflict found.

## Step 4 — Over-privileged grants

Flag universally over-privileged grants: any all-data security context (`*` business unit / ledger), `IT Security Manager` on a non-admin user, `Application Implementation Consultant` (full setup) on an operational user, or a job role that bundles conflicting duties.

## Step 5 — Integration-account review

For `{{scope}} == "system"`: review OIC/integration service accounts and abstract/web-service users. Cross-reference any interactive (human) user being used as an integration technical account — that is a finding.

## Step 6 — Report shape

Produce a markdown report — in this order — and nothing else:

```markdown
# SoD Audit — {{user_or_role}}

## Target
- Pod / environment role
- Scope: {{scope}}
- Audit timestamp: <UTC ISO 8601>

## Findings
| Severity | Code | Title | Evidence |
|---|---|---|---|

## Over-privileged grants
| Role/Privilege | Data scope | Assigned to |
|---|---|---|

## Citations
- oracle-help://... (Oracle SoD / AAC reference)
- oracle-object://PER_USER_ROLES/structure

## Recommendation
- <Action — a change-request / AAC remediation title, never a direct write>
```

**Never** propose to write the fix yourself. SoD remediation requires a security change request, Risk Management approval, and a sandbox/role publish — out of scope for the agent.
