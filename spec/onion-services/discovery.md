> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# Descriptor publication, retrieval, and free discovery

### Ordinary onion-service publication flow

Descriptor publication is separate from maintaining an introduction point. For a normal anonymous onion service:

1. The service's Tor software establishes introduction points, creates the descriptor describing how to contact them, and signs and encrypts it using the v3 onion-service descriptor construction.
2. The service derives the responsible HSDir relays from the current consensus, time-period/blinded-key rules, and shared randomness. These are selected directory relays, not the directory authorities receiving an ordinary relay descriptor.
3. It builds anonymized, normally three-hop upload circuits ending at those HSDirs and uploads descriptor copies. It MUST NOT replace this with a direct connection that exposes the service's origin address.
4. In ordinary Tor, an upload circuit need not remain open merely to keep the descriptor stored. The proposed El Tor lifecycle below normally retains it until its ten-minute expiration. The service continues maintaining separate introduction circuits for forwarding client introductions.
5. A client knowing the onion address derives the appropriate lookup information, fetches a descriptor through Tor, and verifies and decrypts it. Optional client authorization can restrict who can decrypt the connection information.

Descriptors are replicated and refreshed, including when introduction information changes and across descriptor time-period transitions. They are not a single permanent upload or a cleartext network-wide directory listing. HSDirs need not learn the onion address or decrypt introduction details merely to store and serve a valid descriptor. Ordinary clients knowing a non-restricted onion address can retrieve its descriptor.

### El Tor upload-circuit lifetime

After completing publication, normally retain the descriptor-upload circuit until its ten-minute expiration rather than closing it immediately because the upload finished. This follows the shared lifetime policy in spec one, subject to user choice, failures, and free-resource admission/enforcement limits. Remaining idle after a small transfer and closing early when a circuit performs poorly are also normal client behaviors; circuit lifetime or idleness alone is not an onion-service identifier.

Retaining an upload circuit is a lifecycle choice, not a requirement for HSDir storage. Its expiration MUST NOT itself cause an unnecessary new upload or immediate circuit replacement. Publish again when a descriptor update or refresh is needed. Preserve the shared padding/maintenance policy within the free resource budget and count retained idle circuits against the same aggregate limits. Introduction circuits, which must remain available to receive invitations, have their separate replacement requirements below.

### Proposed pricing: bounded free discovery

Basic descriptor publication, storage, refresh, and retrieval SHOULD be available without Lightning payments within explicitly advertised resource limits. This allows a service to become discoverable before its first paying client and allows clients to learn contact and payment information before purchasing rendezvous traffic. It is a proposed policy, not an already implemented free service or an unlimited availability guarantee.

The transport part uses the same purpose-independent bounded free allowance at every hop, including the guard, middle, and HSDir endpoint. Intermediate relays enforce resource limits without needing to inspect descriptor contents or recognize a directory operation. The HSDir separately enforces limits on its storage and request-processing resources. Making endpoint storage free alone would not remove transport charges on the earlier hops.

The allowance must fit legitimate complete descriptor uploads/downloads, acknowledgments, copies, refreshes, and permitted replacement overlap. A descriptor is larger than a single introduction message; a tiny sustained rate may require a bounded burst budget to finish publication in a useful time. Measure this workload before selecting byte rates or burst sizes. Include transport/padding overhead and any compression/decompression limits in the resource accounting. Descriptor size, lifetime, and periodic refresh are distinct from circuit payment periods and the baseline ten-minute expiration of non-renewed circuits.

Free capacity does not silently waive the normal price of an already paid circuit, turn paid application traffic into free traffic, or authorize an automatic wallet charge if a limit is reached. Eligible discovery setup must fit the bounded free setup policy. Additional paid capacity requires explicit operator/client consent and a spending limit; overload should produce bounded retries or temporary unavailability rather than an unexpected charge.

HSDir endpoints cannot be chosen arbitrarily merely because another relay offers free storage: clients must derive the same responsible locations as publishers. Capability signaling and HSDir eligibility/selection rules must be consistent across the network. If a responsible endpoint lacks this feature or is overloaded, use the permitted replicas and bounded retry behavior. Any alternative eligibility rule requires an explicitly specified interoperable protocol change; do not silently change the hash ring on one side.

### Spam and resource-exhaustion controls

Low bandwidth per honest service does not make unrestricted publication safe. An origin can create many service keys, issue repeated descriptor updates, submit malformed documents, or trigger repeated lookups. These consume storage, signature-verification CPU, circuit setup, queues, and response bandwidth. A valid signature proves control of a key, not scarcity of identities or absence of spam.

Supporting HSDirs and path relays MUST implement the following five resource bounds. The policy is recorded here; new numeric defaults remain provisional until legitimate workloads and overload behavior are measured.

| Bound | Initial policy | Scope and rationale |
| --- | --- | --- |
| Descriptor size | Retain Tor's active consensus size limit, currently `HSV3MaxDescriptorSize` with a default of 50,000 bytes. Do not invent a smaller El Tor format. | Bound complete documents and request framing; reject oversized/malformed inputs without unbounded allocation. Preserve descriptor encryption and padding rules. |
| Upload/update rate | Allow a bounded burst, then a configurable sustained update rate per descriptor key, with explicit retry/backoff behavior. | Accommodate introduction replacement, legitimate revisions, replicas, overlapping periods, and recovery. Repeated uploads or new keys must not bypass global work limits. |
| Total storage | Use a configurable HSDir cache/memory budget, expire entries, and retain the appropriate latest revision rather than unlimited revision history. | Validate descriptor certificates/signatures and revision rules. Do not overwrite a newer accepted revision with an older one. Establish bounded overload/eviction behavior. |
| Free traffic | Bound sustained and burst traffic per circuit and across the relay, in both directions, including responses, padding, and accounting overhead. | A burst must fit a complete valid descriptor transfer. Cap repeated lookups/misses and combined traffic across circuits; the free allowance is not multiplied without limit by opening more circuits. |
| Setup and processing | Bound circuit creation, concurrent uploads/downloads, verification work, queued requests, pending builds, and buffering. | Tiny transfers still consume CPU and memory. Enforce relay-wide ceilings across connections and newly generated identities. |

Relay-local resource budgets should be configurable, with interoperable advertised free-capacity limits and predictable rejection/retry behavior. Retaining a Tor consensus size limit does not mean that every other quota becomes a consensus parameter. Clients and services must have enough information to select a usable path without exposing their onion identity to intermediate relays. Test defaults against actual publication and retrieval workloads, especially our proposed ten-minute introduction-circuit replacements, before selecting update rates, free byte rates, burst sizes, or concurrency/cache budgets.

Keep the outer descriptor verifiable without requiring HSDirs to decrypt its connection information, learn the long-term onion identity, or link successive blinded identities solely to enforce quotas. Per-key and per-connection throttles supplement global limits; they cannot prevent one origin from creating many identities. Fair admission under a flood remains a design and test topic. Limits bound resource exposure but do not promise that spam cannot make a service temporarily unreachable.

### Validation and open decisions

Measure publication size, replica count, update rate, and retrieval traffic for legitimate services, especially with ten-minute introduction-circuit replacement and time-period overlap. Test that an onion service can publish, refresh, and be found through an entirely eligible free path without paying continuous relay fees or exposing its origin. Confirm that clients and publishers agree on responsible HSDirs and reject stale/invalid descriptors.

Then test many-key upload floods, frequent revisions, malformed/oversized inputs, repeated lookups/misses, and combined traffic across circuits and connections. Verify bounded storage, CPU, queues, setup work, and bandwidth, as well as recovery for legitimate users after overload. Numeric quotas, fair admission, capability encoding, and any extra anti-spam mechanism remain undecided until these tests inform them. This section records a bounded-free proposal, not completed spam-resistance validation.
