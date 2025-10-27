# Backup Circuit Implementation with Round-Robin Load Balancing

## Overview

The backup circuit feature provides both **load balancing** and **redundancy** for the eltord client. Instead of waiting for the primary circuit to fail, both circuits are used simultaneously in a **round-robin fashion**, alternating between them for each payment round. This distributes the load evenly and provides seamless failover if one circuit experiences issues.

## How It Works

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    eltord Client                            │
│                                                             │
│  ┌──────────────┐              ┌──────────────┐           │
│  │   PRIMARY    │              │    BACKUP    │           │
│  │  Circuit 123 │              │  Circuit 124 │           │
│  │              │              │              │           │
│  │ Guard → Mid  │              │ Guard → Mid  │           │
│  │    → Exit    │              │    → Exit    │           │
│  └──────┬───────┘              └──────┬───────┘           │
│         │                             │                    │
│         └──────────┬──────────────────┘                    │
│                    │                                        │
│              Round-Robin                                    │
│         ┌──────────┴──────────┐                           │
│         ▼                     ▼                            │
│    Round 1, 3, 5, 7, 9   Round 2, 4, 6, 8, 10            │
│    (PRIMARY)              (BACKUP)                         │
└─────────────────────────────────────────────────────────────┘
```

### Payment Round Distribution

| Round | Circuit Used | Relays Paid | Bandwidth Check |
|-------|--------------|-------------|-----------------|
| 1     | PRIMARY 123  | 3 relays    | ✓ Before payment |
| 2     | BACKUP 124   | 3 relays    | ✓ Before payment |
| 3     | PRIMARY 123  | 3 relays    | ✓ Before payment |
| 4     | BACKUP 124   | 3 relays    | ✓ Before payment |
| 5     | PRIMARY 123  | 3 relays    | ✓ Before payment |
| 6     | BACKUP 124   | 3 relays    | ✓ Before payment |
| 7     | PRIMARY 123  | 3 relays    | ✓ Before payment |
| 8     | BACKUP 124   | 3 relays    | ✓ Before payment |
| 9     | PRIMARY 123  | 3 relays    | ✓ Before payment |
| 10    | BACKUP 124   | 3 relays    | ✓ Before payment |

**Total**: 30 relay payments across 2 circuits (15 payments each)

### 1. Relay Selection (Step 2b)
After selecting relays for the primary circuit, the client:
- Calls `simple_relay_selection_algo()` again to get a different set of relays
- Ensures diversity by selecting from the available relay pool
- Logs the backup relay selection for debugging

### 2. Payment Hash Pregeneration (Step 4b)
- Generates payment ID hashes for the backup circuit relays
- Same number of rounds as the primary circuit (default: 10)
- Each relay gets unique payment hashes for verification

### 3. Circuit Building (Step 5b)
The backup circuit is built immediately after the primary circuit:
- Uses `EXTENDPAIDCIRCUIT` command with backup relays
- Waits for the circuit to reach `BUILT` state (up to 30 seconds)
- Logs success/failure of backup circuit build
- If backup build fails, continues with primary circuit only

### 4. Payment Loop with Round-Robin Load Balancing (Step 7)
The client uses both circuits simultaneously in a round-robin pattern:

**Round-Robin Strategy:**
- **Round 1**: Use PRIMARY circuit → Pay all relays in primary circuit
- **Round 2**: Use BACKUP circuit → Pay all relays in backup circuit  
- **Round 3**: Use PRIMARY circuit → Continue alternating...
- **Round 4**: Use BACKUP circuit
- And so on for 10 rounds total

**Benefits:**
- **Load Distribution**: Each circuit handles 50% of the rounds
- **Better Performance**: Distributes network load across multiple paths
- **Automatic Failover**: If one circuit fails during its round, the other circuit continues
- **Higher Throughput**: Two circuits can handle more streams combined

**Monitoring:**
- Bandwidth checks every 2 seconds via SOCKS proxy
- Stream capacity monitoring (warns at 256 streams per circuit)
- If bandwidth fails on one circuit's round, continues with next circuit

## Benefits

1. **Load Balancing**: Distributes traffic evenly across both circuits (50/50 split)
2. **Higher Throughput**: Two circuits can handle more concurrent streams (up to 512 combined)
3. **Better Performance**: Multiple network paths reduce congestion
4. **Higher Availability**: If one circuit fails, the other continues seamlessly
5. **Cost Efficiency**: Both paid circuits are actively used, not just held in reserve
6. **Seamless Recovery**: No manual intervention required
7. **Better User Experience**: More consistent performance and fewer interruptions
8. **Tor Best Practice**: Mimics Tor's multi-circuit approach

## Configuration

The backup circuit uses the same configuration as primary:
- `PAYMENT_INTERVAL_ROUNDS` - Number of payment rounds (default: 10)
- `PaymentCircuitMaxFee` - Maximum fee for circuit selection
- `EntryNodes`/`ExitNodes` - Relay preferences apply to both circuits

## Code Changes

### Modified Files

1. **`src/client/start_client_flow.rs`**
   - Added backup relay selection (step 2b)
   - Added backup payment hash generation (step 4b)
   - Added backup circuit build (step 5b)
   - Added failover logic in payment loop (step 7)

2. **`src/client/payments_loop.rs`**
   - Changed wallet parameter to `Arc<Box<dyn LightningNode>>` for sharing
   - Updated error types to include `Send + Sync` bounds
   - Function signature now supports being called multiple times with same wallet

3. **`src/client/circuit.rs`**
   - Updated error type to include `Send + Sync` bounds

### Key Implementation Details

**Round-Robin Algorithm:**
```rust
// Determine which circuit to use (odd rounds = primary, even rounds = backup)
let (current_relays, current_circuit_id, circuit_name) = if round % 2 == 1 {
    (primary_relays, primary_circuit_id, "PRIMARY")
} else {
    (backup_relays, backup_circuit_id, "BACKUP")
};
```

**New Function:**
- `start_payments_loop_round_robin()`: Orchestrates alternating payment rounds
- Takes both primary and backup circuit parameters
- Alternates between circuits using modulo arithmetic (round % 2)
- Shares the Lightning wallet via `Arc` between both circuits
- Each round processes all 3 relays in the current circuit before switching

## Example Log Output

```
[INFO] Selecting relays for backup circuit...
[INFO] Backup circuit relays: [Relay { nickname: "GuardRelay2", ... }, ...]
[INFO] Building backup circuit...
[INFO] Created backup Circuit with ID: 124
[INFO] Waiting for backup circuit 124 to be fully built...
[INFO] ✅ Backup circuit 124 is BUILT and ready!
[INFO] ✅ Primary circuit 123 is BUILT and ready for traffic!
[INFO] ✅ Backup circuit is also BUILT - using ROUND-ROBIN load balancing!
[INFO] 🔄 Starting round-robin load balancing between circuits 123 and 124
[INFO] 🔄 Starting round-robin payment loop with 10 rounds
[INFO]    Primary circuit: 123
[INFO]    Backup circuit: 124
[INFO] 🥊 Round 1/10 - Using PRIMARY circuit 123 🥊
[INFO] 🛜  SOCKS bandwidth check passed before payment round 1 on PRIMARY circuit (45 total streams)
[INFO] Paying 100 sats relay: ...
[INFO] 🥊 Round 2/10 - Using BACKUP circuit 124 🥊
[INFO] 🛜  SOCKS bandwidth check passed before payment round 2 on BACKUP circuit (47 total streams)
[INFO] Paying 100 sats relay: ...
[INFO] 🥊 Round 3/10 - Using PRIMARY circuit 123 🥊
... (alternates for 10 rounds) ...
[INFO] ✅ Round-robin payment loops completed successfully for both circuits!
```

## Failure Handling Details

**Single Circuit Failure:**
- If one circuit loses bandwidth during its round, the system automatically fails over to the other circuit
- The failover circuit is checked for bandwidth before proceeding
- If the failover succeeds, payments continue on the alternate circuit for that round
- Operation continues in degraded mode with only one active circuit

**Simultaneous Failure Edge Case:**
- If **both circuits lose bandwidth simultaneously**, the client aborts and the current run stops
- The system does not yet perform automatic background circuit rebuilds
- The client will restart and build fresh circuits on the next loop iteration
- This is a rare scenario (requires both network paths to fail at the same moment)

**Recommended Mitigation:**
- Monitor logs for bandwidth failures and investigate network connectivity
- Consider building 3+ circuits in the future for better redundancy

## Future Improvements

1. **Circuit Pool**: Build multiple circuits (3-4 circuits total) for more distribution
2. **Proactive Rebuilding**: Rebuild failed circuits in background while others continue (auto-recovery from simultaneous failures)
3. **Weighted Round-Robin**: Adjust distribution based on circuit performance
4. **Circuit Metrics**: Track success rates, latency, and throughput per circuit
5. **Smart Selection**: Prefer relays with better historical performance
6. **Dynamic Failover**: Skip failed circuits automatically in rotation
7. **Per-Stream Load Balancing**: Route individual streams to least-loaded circuit via ATTACHSTREAM control command

## Testing

To test the backup circuit and round-robin load balancing:

1. Start the client normally
2. Observe two circuits being built in the logs
3. Watch the round-robin alternation between PRIMARY and BACKUP circuits
4. Generate traffic (e.g., connect browser to SOCKS proxy) and verify both circuits handle streams
5. Monitor `data/payments_sent.json` to confirm payments to relays in both circuits

**Testing Failover:**
- To test single-circuit failure recovery, you can simulate primary circuit failure (e.g., temporarily disconnect network or kill one relay process)
- Verify automatic failover to backup circuit in the logs
- Check that browsing continues without manual intervention

**Note on Simultaneous Failures:**
- If both circuits fail bandwidth checks at the same time, the client will abort the current run
- The outer loop in `start_client_flow` will automatically restart and rebuild fresh circuits
- Monitor logs for "Both circuits have lost bandwidth" to diagnose network issues

## Notes

- Backup circuit uses the same Lightning wallet instance (shared via Arc)
- Both circuits are paid circuits with separate payment schedules
- If no suitable backup relays found, client continues with primary only
- The implementation follows Tor's design philosophy of having backup circuits
