# SYNERGY AUTOPILOT V5 AI Node — LAB Deployment Reference v1

This additive generation closes the evidence loop back into the canonical node owner without mutating the original AUTOPILOT V5 integration release root.

## Bound roots

- integration manifest: `a793250aaa2299fdaf1661e985423c2b62687a19395ceabcc7bb63c005defe54`
- release roots: `224e6c43bfccb3b0db7acf92f243dace0ee5228a7ab31a0be140328e692f3630`
- SynergyNet transport binding: `3c8198e10863c52c0492ca422c404a1e05e6fd2f48f4c393587d9d87eee3855f`
- Meta-OS Profile v2: `6a84a2dd132823756c9f159b04584ac384dca9514316b3ff5c9f4ce06e1696d3`
- Meta-OS Admission v2: `4d3502dba28745b8a734c082ca0260cbbfd4ead11a1396537434844df9987ee3`
- LAB HostProfile: `36a9092977e360b56fd7d1e21b4504f1215ed741df6f492b3ed37b79649573bb`
- LAB SupervisorConfig: `74797e02e0bb32722bbfffb76af42bb00d495fb3a5c0ea4a5b48a4e1d4275879`
- LAB DeploymentEvidence: `8e1b05dca7e349d57e1908dca717d2c1d451ca79adde5cfe539e9e46f83d052b`
- LAB DeploymentResult: `011c3ce24cfd33dc99fbdea0ea2f5dcd344deefd927f57c1b9c1fa3e3eaa3833`
- SYNERGY System Registry v2: `12bdca330d81b81ec0acb32cf974f12fb36c5e72e6a1fea8680dc5e8470b275d`
- AI Blockchain deployment commitment v2: `1b47ae40ae48848be1648da5cc1c443232a47dc59969e70a6dd033672e5397c0`
- Meta-OS Production Transition Gate: `fd3f97c1422376ae507e7ec0ab1dcff529cba4ad9b5f6b1e9b7b62ef43d75b8c`

DeploymentReference v1 root:

`19375c02a286a4c14e704ad0496d1c6084ddb00e7c4bcf3506a304a6b0946b70`

## Claim boundary

The reference records only the executed ephemeral LAB lifecycle:

`RUNNING_VERIFIED -> STOPPED_AFTER_VERIFICATION`.

It explicitly preserves `current_running=false` and `production_status=BLOCKED_NOT_DEPLOYED`. It is not production evidence and cannot grant financial, payment, accounting, blockchain-submission or execution authority.

## Verification model

The self-digest is SHA-256 over canonical JSON excluding the `deployment_reference_digest` field. The verifier also requires every pinned cross-domain root and state/authority field to equal its exact expected value. Re-hashing a mutated object therefore does not make a substituted upstream root or promoted state valid.
