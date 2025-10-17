SPEEDUP_TOR_BOOTSTRAP
====================
## Tor Circuit Build Timeout Configuration Guide

## Overview

This document explains how `LearnCircuitBuildTimeout` works in Tor and how to optimize it for different use cases.

## Table of Contents

- [What is LearnCircuitBuildTimeout?](#what-is-learncircuitbuildtimeout)
- [How Learning Works](#how-learning-works)
- [Timeline Comparison](#timeline-comparison)
- [The Learning Algorithm](#the-learning-algorithm)
- [Trade-offs](#trade-offs)
- [Configuration Recommendations](#configuration-recommendations)
- [Examples](#examples)

---

## What is LearnCircuitBuildTimeout?

`LearnCircuitBuildTimeout` controls whether Tor should **dynamically learn** the optimal circuit build timeout by observing actual circuit construction times in your network.

### Default Behavior (LearnCircuitBuildTimeout 1)

```
┌─────────────────────────────────────────────────────────────┐
│ LEARNING PHASE (can take 5-30+ seconds)                     │
├─────────────────────────────────────────────────────────────┤
│ 1. Tor starts up                                            │
│ 2. Downloads descriptors                                    │
│ 3. Attempts to build test circuits                          │
│ 4. Measures: "How long did each circuit take to build?"     │
│    - Circuit 1: 1.2s ✓                                      │
│    - Circuit 2: 0.8s ✓                                      │
│    - Circuit 3: 2.1s ✓                                      │
│    - Circuit 4: 1.5s ✓                                      │
│                                                              │
│ 5. Statistical analysis:                                    │
│    - Calculate median build time                            │
│    - Calculate variance                                     │
│    - Apply quantile-based algorithm                         │
│    - Result: "Optimal timeout = 15 seconds"                 │
│                                                              │
│ 6. NOW ready to build circuits for real traffic             │
└─────────────────────────────────────────────────────────────┘
```

### With Learning Disabled (LearnCircuitBuildTimeout 0)

```
┌─────────────────────────────────────────────────────────────┐
│ NO LEARNING - IMMEDIATE START                               │
├─────────────────────────────────────────────────────────────┤
│ 1. Tor starts up                                            │
│ 2. Downloads descriptors                                    │
│ 3. IMMEDIATELY starts building circuits for real traffic    │
│ 4. Uses fixed timeout (CircuitBuildTimeout setting)         │
│    - Default: 60 seconds                                    │
│    - Custom: 10 seconds (for test networks)                 │
│                                                              │
│ NO WAITING, NO MEASURING, NO STATISTICAL ANALYSIS           │
└─────────────────────────────────────────────────────────────┘
```

---

## How Learning Works

Tor uses a **quantile-based estimator** for circuit build timeouts:

```rust
// Pseudocode of Tor's algorithm
struct CircuitBuildTimeEstimator {
    build_times: Vec<Duration>,  // Recent circuit build times
    timeout: Duration,            // Current learned timeout
}

impl CircuitBuildTimeEstimator {
    fn add_sample(&mut self, build_time: Duration) {
        // Keep last 100 samples
        self.build_times.push(build_time);
        if self.build_times.len() > 100 {
            self.build_times.remove(0);
        }
        
        // Need at least 20 samples to learn
        if self.build_times.len() < 20 {
            return; // Not enough data yet
        }
        
        // Calculate quantile-based timeout
        let mut sorted = self.build_times.clone();
        sorted.sort();
        
        // Use 80th percentile (Xm)
        let quantile_idx = (sorted.len() as f64 * 0.80) as usize;
        let quantile_time = sorted[quantile_idx];
        
        // Add safety margin (α = 1.5)
        self.timeout = quantile_time * 1.5;
        
        // Enforce minimum (3s) and maximum (60s)
        self.timeout = self.timeout.clamp(3.0, 60.0);
    }
}
```

### Algorithm Details

- **Samples collected**: 20-100 recent circuit build times
- **Percentile used**: 80th percentile (20% of circuits may be slower)
- **Safety margin**: 1.5x multiplier
- **Range**: Clamped between 3s and 60s

### Why 80th Percentile?

- **80%** of circuits will complete within this time
- **20%** might be slow (distant relays, congestion)
- Better than mean (affected by outliers)
- Better than median (too aggressive, 50% would fail)

### Why 1.5x Safety Margin?

- Network conditions vary
- Occasional congestion spikes
- Different relay speeds
- Buffer against measurement error

---

## Timeline Comparison

### With Learning (LearnCircuitBuildTimeout 1)

```
T+0.0s: Tor process starts
T+0.5s: Read torrc, start listening on ports
T+1.0s: Connect to directory authorities
T+2.0s: Download consensus (network status document)
T+3.0s: Download relay descriptors
T+4.0s: Parse descriptors, build internal routing tables
T+5.0s: ┌─────────────────────────────────────────┐
        │ START LEARNING PHASE                    │
        ├─────────────────────────────────────────┤
        │ Build test circuit #1                   │
T+6.0s: │ - Select guard, middle, exit            │
T+6.5s: │ - Extend to guard... (0.5s)             │
T+7.0s: │ - Extend to middle... (0.5s)            │
T+7.5s: │ - Extend to exit... (0.5s)              │
T+8.0s: │ ✓ Circuit 1 built (took 2.0s)           │
        │                                         │
T+8.5s: │ Build test circuit #2                   │
T+9.0s: │ ...measuring...                         │
T+10.0s:│ ✓ Circuit 2 built (took 1.5s)           │
        │                                         │
T+11.0s:│ Build test circuit #3                   │
T+13.0s:│ ✓ Circuit 3 built (took 2.0s)           │
        │                                         │
T+14.0s:│ Collect enough samples...               │
T+20.0s:│ Statistical analysis...                 │
T+21.0s:│ Calculate 80th percentile: 1.8s         │
T+22.0s:│ Add safety margin: 1.8s × 1.5 = 2.7s    │
T+23.0s:│ Round up for safety: 15s timeout        │
T+24.0s:│ ✓ Learning complete!                    │
        └─────────────────────────────────────────┘
T+25.0s: Ready for SOCKS connections
T+26.0s: ✅ USER CAN CONNECT

Total time: 26 seconds
```

### Without Learning (LearnCircuitBuildTimeout 0)

```
T+0.0s: Tor process starts
T+0.5s: Read torrc, start listening on ports
T+1.0s: Connect to directory authorities
T+2.0s: Download consensus
T+3.0s: Download relay descriptors
T+4.0s: Parse descriptors
T+5.0s: ✅ IMMEDIATELY READY - No learning phase!
T+5.0s: Start building circuits for real traffic
T+5.5s: Circuit built
T+6.0s: ✅ USER CAN CONNECT

Total time: 6 seconds
Time saved: 20 seconds! 🚀
```

---

## The Learning Algorithm

### Real-World Example

```
Sample build times collected:
[0.8s, 1.1s, 0.9s, 1.5s, 1.2s, 2.1s, 1.0s, 1.3s, 0.7s, 1.8s, 
 1.4s, 1.1s, 1.6s, 1.2s, 0.9s, 1.7s, 1.3s, 1.0s, 1.5s, 2.0s]

Step 1: Sort the times
Sorted: [0.7, 0.8, 0.9, 0.9, 1.0, 1.0, 1.1, 1.1, 1.2, 1.2,
         1.3, 1.3, 1.4, 1.5, 1.5, 1.6, 1.7, 1.8, 2.0, 2.1]

Step 2: Find 80th percentile
80th percentile (16th value out of 20): 1.6s

Step 3: Apply safety margin
With 1.5x margin: 1.6 × 1.5 = 2.4s

Step 4: Round up for safety
Rounded up: 15s (Tor's minimum for public network)

Result: CircuitBuildTimeout = 15s
```

### From Tor Source Code

```c
// Simplified version from circuitbuild.c
int learned_timeout = quantile(circuit_build_times, 0.80) * 1.5;
```

---

## Trade-offs

### Advantages of Learning (LearnCircuitBuildTimeout 1)

| Advantage | Description |
|-----------|-------------|
| ✅ **Optimal timeout** | Adapts to your specific network conditions |
| ✅ **Better for slow networks** | Won't timeout circuits prematurely |
| ✅ **Network-aware** | If network gets slower, timeout increases automatically |
| ✅ **Tor Browser default** | Proven safe for millions of users |
| ✅ **Adaptive** | Adjusts to changing network conditions |

### Disadvantages of Learning (LearnCircuitBuildTimeout 1)

| Disadvantage | Description |
|--------------|-------------|
| ❌ **Slow startup** | 20-30 second delay before ready |
| ❌ **Extra circuits** | Builds test circuits that aren't used |
| ❌ **Wasted bandwidth** | Learning circuits consume bandwidth |
| ❌ **Delay for user** | User waits even with cached descriptors |
| ❌ **Resource overhead** | CPU/memory for statistical analysis |

### Advantages of Not Learning (LearnCircuitBuildTimeout 0)

| Advantage | Description |
|-----------|-------------|
| ✅ **Instant startup** | Ready in 2-6 seconds |
| ✅ **No test circuits** | All circuits serve real traffic |
| ✅ **Lower bandwidth** | No learning overhead |
| ✅ **Better UX** | User doesn't wait |
| ✅ **Perfect for cached descriptors** | You already know the network |
| ✅ **Predictable** | Fixed timeout, no surprises |

### Disadvantages of Not Learning (LearnCircuitBuildTimeout 0)

| Disadvantage | Description |
|--------------|-------------|
| ❌ **Fixed timeout** | Might be too short/long for current network |
| ❌ **Manual tuning** | Need to set CircuitBuildTimeout yourself |
| ❌ **Less adaptive** | Won't adjust if network gets slower |
| ❌ **Could timeout good circuits** | If timeout too aggressive |
| ❌ **Maintenance required** | Need to update if network changes |

---

## Configuration Recommendations

```toml
# LearnCircuitBuildTimeout = Don't wait to "learn" optimal timeouts, just build circuits immediately with whatever descriptors you have
# CircuitBuildTimeout = Fixed timeout when not learning (10s for test networks, 30-60s for production)
# These settings speed up boot from 5-30 seconds to only 2 seconds
# Use this setting only if you have a background service download new descriptors every hour for security purposes
LearnCircuitBuildTimeout 0
CircuitBuildTimeout 10                   
UseMicrodescriptors 1
# Skip downloads on bootstrap
FetchDirInfoEarly 0
# No pre-fetching            
FetchDirInfoExtraEarly 0
# Accept 7 days old descriptors
TestingDirConnectionMaxStall 604800
```
---

## Quick Decision Guide

**Use `LearnCircuitBuildTimeout 0` when:**
- ✅ Test network
- ✅ Known/stable network conditions
- ✅ Fast startup is critical
- ✅ Cached descriptors available
- ✅ You can set appropriate CircuitBuildTimeout manually
- ✅ Network characteristics don't change
- ✅ All relays on fast/predictable network (LAN, datacenter)

**Use `LearnCircuitBuildTimeout 1` when:**
- ✅ Public Tor network
- ✅ Unknown network conditions
- ✅ Varying relay speeds (worldwide relays)
- ✅ Mobile networks (changing conditions)
- ✅ First run (no cached data)
- ✅ Network conditions change frequently
- ✅ You want "set and forget" configuration

---

## Additional Resources

### Tor Documentation
- [Tor Manual - Circuit Timeouts](https://2019.www.torproject.org/docs/tor-manual.html.en)
- [Tor Specification - Path Selection](https://spec.torproject.org/path-spec/index.html)

### Related Settings
- `CircuitBuildTimeout` - Fixed timeout when not learning
- `UseMicrodescriptors` - Use smaller relay descriptors (faster download)
- `FetchDirInfoEarly` - Aggressively fetch directory information
- `TestingDirConnectionMaxStall` - Allow stale descriptors in test networks
