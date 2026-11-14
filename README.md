# man0 NEAR Programs

Native NEAR Rust smart contracts for the **man0** protocol. Start with the [project overview](https://github.com/man0-space/submission) and [judge walkthrough](https://github.com/man0-space/submission/blob/main/JUDGES.md).

## Workspace Layout

- `crates/man0`: Shared types, 18-decimal WAD fixed-point arithmetic, SHA-256 deterministic DNA & Warp hash derivation, metadata validation, NEP-297 events (`standard = "man0"` and `standard = "nep171"`), storage staking utilities, and cross-contract `#[ext_contract]` traits.
- `contracts/factory`: ERC-8004-style Agent Factory & Registry, NEP-171/177/178/181 Agent NFTs, cloning, warp records, and license consumption/revocation.
- `contracts/reputation`: ERC-8004-style Subjective Trust Registry (client feedback, anti-self-feedback enforcement, revocation, responses, and 18-decimal WAD summary aggregation with mode decimals).
- `contracts/validation`: ERC-8004-style Objective Trust Registry (validation requests, validator attestations, and requester closure).
- `contracts/workflow`: ERC-7401-style Nestable Workflow NFTs (NEP-171/177/178/181 + parent-child nesting), RFA (Request-For-Agent) escrow, lease lifecycle, and NEP-141 (`ft_on_transfer`) USDC settlement.

## Deployed Testnet Program IDs

| Program | NEAR Testnet Account ID | Deployment Transaction |
| :--- | :--- | :--- |
| `factory` | `factory.man0u.testnet` | `69V9QzqYSEBo5JhbjoirDSBfi8PARSFN4DnT5vAoXKKh` |
| `reputation` | `reputation.man0u.testnet` | `E5tGBcT4zmG2xFXFEZfNLe8rCcDBKq9JuHqS6UQoFe2j` |
| `validation` | `validation.man0u.testnet` | `6EwXNFq6y2bEd13ffeir9EpTPCEcWQaKyRuECpDrvGeP` |
| `workflow` | `workflow.man0u.testnet` | `AMUKNAwS6ERtgZeDa8LSccKSWEmsBkT62m1aPbUdv5Mg` |

## Build & Test

```bash
# Run unit and integration tests
cargo test --workspace

# Build optimized WASM binaries with cargo-near
for prog in factory reputation validation workflow; do
  (cd "contracts/$prog" && cargo near build non-reproducible-wasm --no-abi)
done
```
