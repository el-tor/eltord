#!/bin/bash
# Test SOCKS5 proxy timing to identify when it becomes ready
# This script tests the SOCKS5 proxy every 2 seconds for 60 seconds
# to determine exactly when it starts accepting connections after circuit build

SOCKS_PORT=${1:-18058}  # Default port from torrc, can override with argument
CONTROL_PORT=${2:-9992}  # Tor control port, can override
CONTROL_PASSWORD=${3:-password1234_}  # Control password
SOCKS_HOST="127.0.0.1"
TEST_HOST="www.google.com"
TEST_PORT=80
INTERVAL=2  # Test every 2 seconds
DURATION=3600 # Test for 60 minutes total

echo "🧪 SOCKS5 Proxy Timing Test with Traffic Monitoring"
echo "======================================"
echo "Testing: $SOCKS_HOST:$SOCKS_PORT"
echo "Control: $SOCKS_HOST:$CONTROL_PORT"
echo "Target: $TEST_HOST:$TEST_PORT"
echo "Interval: ${INTERVAL}s"
echo "Duration: ${DURATION}s"
echo "======================================"
echo ""

# Function to get traffic stats from Tor
get_traffic_stats() {
    RESPONSE=$(echo -e "AUTHENTICATE \"$CONTROL_PASSWORD\"\r\nGETINFO traffic/read\r\nGETINFO traffic/written\r\nQUIT\r\n" | nc $SOCKS_HOST $CONTROL_PORT 2>/dev/null)
    
    BYTES_READ=$(echo "$RESPONSE" | grep "traffic/read=" | sed 's/.*traffic\/read=//' | tr -d '\r')
    BYTES_WRITTEN=$(echo "$RESPONSE" | grep "traffic/written=" | sed 's/.*traffic\/written=//' | tr -d '\r')
    
    # Return values (use global variables since bash functions can't return multiple values easily)
    TRAFFIC_READ=$BYTES_READ
    TRAFFIC_WRITTEN=$BYTES_WRITTEN
}

# Function to get circuit status (shows what circuits are being built/used)
get_circuit_status() {
    RESPONSE=$(echo -e "AUTHENTICATE \"$CONTROL_PASSWORD\"\r\nGETINFO circuit-status\r\nQUIT\r\n" | nc $SOCKS_HOST $CONTROL_PORT 2>/dev/null)
    
    # Count circuits by state
    LAUNCHED=$(echo "$RESPONSE" | grep -c "LAUNCHED" || echo "0")
    BUILDING=$(echo "$RESPONSE" | grep -c "BUILDING" || echo "0")
    BUILT=$(echo "$RESPONSE" | grep -c "BUILT" || echo "0")
    FAILED=$(echo "$RESPONSE" | grep -c "FAILED" || echo "0")
    
    CIRCUIT_LAUNCHED=$LAUNCHED
    CIRCUIT_BUILDING=$BUILDING
    CIRCUIT_BUILT=$BUILT
    CIRCUIT_FAILED=$FAILED
    
    # Store full circuit details for detailed display
    CIRCUIT_DETAILS="$RESPONSE"
}

# Function to get detailed circuit info (ID, state, purpose, path)
get_circuit_details() {
    echo "$CIRCUIT_DETAILS" | grep "^250" | while read -r line; do
        # Parse circuit line format: "250-{CircuitID} {State} {Path} {BuildFlags} {Purpose} {HSState} {RendQuery} {TimeCreated} {Reason} {RemoteReason}"
        # Example: "250-52 BUILT $ABC~relay1,$DEF~relay2,$GHI~relay3 BUILD_FLAGS=IS_INTERNAL,NEED_CAPACITY PURPOSE=GENERAL TIME_CREATED=2025-10-15T10:30:00.000000"
        
        CIRC_ID=$(echo "$line" | awk '{print $1}' | sed 's/250-//' | sed 's/250 //')
        STATE=$(echo "$line" | awk '{print $2}')
        PATH=$(echo "$line" | awk '{print $3}' | sed 's/\$[^~]*~/→/g' | sed 's/,/ /g')
        
        # Extract PURPOSE if present
        if echo "$line" | grep -q "PURPOSE="; then
            PURPOSE=$(echo "$line" | grep -o "PURPOSE=[^ ]*" | sed 's/PURPOSE=//')
        else
            PURPOSE="UNKNOWN"
        fi
        
        # Count hops in path
        HOPS=$(echo "$PATH" | grep -o "→" | wc -l | tr -d ' ')
        HOPS=$((HOPS + 1))
        
        echo "    Circuit $CIRC_ID: $STATE | Purpose: $PURPOSE | Hops: $HOPS | Path: ${PATH:0:50}..."
    done
}

# Function to get stream status (shows active SOCKS connections)
get_stream_status() {
    RESPONSE=$(echo -e "AUTHENTICATE \"$CONTROL_PASSWORD\"\r\nGETINFO stream-status\r\nQUIT\r\n" | nc $SOCKS_HOST $CONTROL_PORT 2>/dev/null)
    
    # Count streams by state
    STREAM_NEW=$(echo "$RESPONSE" | grep -c "NEW" || echo "0")
    STREAM_SENTCONNECT=$(echo "$RESPONSE" | grep -c "SENTCONNECT" || echo "0")
    STREAM_SUCCEEDED=$(echo "$RESPONSE" | grep -c "SUCCEEDED" || echo "0")
    STREAM_FAILED=$(echo "$RESPONSE" | grep -c "FAILED" || echo "0")
    
    STREAMS_NEW=$STREAM_NEW
    STREAMS_SENTCONNECT=$STREAM_SENTCONNECT
    STREAMS_SUCCEEDED=$STREAM_SUCCEEDED
    STREAMS_FAILED=$STREAM_FAILED
}

# Function to get bootstrap status (explains what Tor is doing)
get_bootstrap_status() {
    RESPONSE=$(echo -e "AUTHENTICATE \"$CONTROL_PASSWORD\"\r\nGETINFO status/bootstrap-phase\r\nQUIT\r\n" | nc $SOCKS_HOST $CONTROL_PORT 2>/dev/null)
    
    # Extract bootstrap percentage and summary
    BOOTSTRAP_PCT=$(echo "$RESPONSE" | grep "BOOTSTRAP PROGRESS=" | sed 's/.*PROGRESS=//' | sed 's/ .*//' || echo "0")
    BOOTSTRAP_TAG=$(echo "$RESPONSE" | grep "TAG=" | sed 's/.*TAG=//' | sed 's/ .*//' | tr -d '\r' || echo "unknown")
    BOOTSTRAP_SUMMARY=$(echo "$RESPONSE" | grep "SUMMARY=" | sed 's/.*SUMMARY=//' | tr -d '\r' | sed 's/"//g' || echo "unknown")
    
    BOOTSTRAP_PROGRESS=$BOOTSTRAP_PCT
    BOOTSTRAP_STATUS_TAG=$BOOTSTRAP_TAG
    BOOTSTRAP_STATUS_SUMMARY=$BOOTSTRAP_SUMMARY
}

# Function to get dormant status (is Tor active or sleeping?)
get_dormant_status() {
    RESPONSE=$(echo -e "AUTHENTICATE \"$CONTROL_PASSWORD\"\r\nGETINFO dormant\r\nQUIT\r\n" | nc $SOCKS_HOST $CONTROL_PORT 2>/dev/null)
    
    IS_DORMANT=$(echo "$RESPONSE" | grep "dormant=" | sed 's/.*dormant=//' | tr -d '\r' || echo "0")
    
    DORMANT=$IS_DORMANT
}

# Check if port is listening
echo "🔍 Checking if SOCKS port is listening..."
if lsof -i :$SOCKS_PORT > /dev/null 2>&1; then
    echo "✅ Port $SOCKS_PORT is listening"
    lsof -i :$SOCKS_PORT | grep LISTEN
else
    echo "❌ Port $SOCKS_PORT is NOT listening!"
    echo "Make sure Tor/ELTOR is running with SOCKS port configured"
    exit 1
fi

# Check if control port is accessible
echo ""
echo "🔍 Checking Tor control port..."
if nc -z $SOCKS_HOST $CONTROL_PORT 2>/dev/null; then
    echo "✅ Control port $CONTROL_PORT is accessible"
    
    # Get initial traffic stats
    get_traffic_stats
    INITIAL_READ=$TRAFFIC_READ
    INITIAL_WRITTEN=$TRAFFIC_WRITTEN
    
    if [ -n "$INITIAL_READ" ] && [ -n "$INITIAL_WRITTEN" ]; then
        echo "📊 Initial traffic stats:"
        echo "   Read: $(numfmt --to=iec $INITIAL_READ 2>/dev/null || echo $INITIAL_READ) bytes"
        echo "   Written: $(numfmt --to=iec $INITIAL_WRITTEN 2>/dev/null || echo $INITIAL_WRITTEN) bytes"
    else
        echo "⚠️  Could not get traffic stats (authentication may have failed)"
        INITIAL_READ=0
        INITIAL_WRITTEN=0
    fi
    
    # Get initial circuit/stream/bootstrap status
    get_circuit_status
    echo "🔄 Initial circuit status: BUILT=$CIRCUIT_BUILT, BUILDING=$CIRCUIT_BUILDING, LAUNCHED=$CIRCUIT_LAUNCHED, FAILED=$CIRCUIT_FAILED"
    
    if [ "$CIRCUIT_BUILT" != "0" ] || [ "$CIRCUIT_BUILDING" != "0" ]; then
        echo ""
        echo "  📋 Circuit Details:"
        get_circuit_details
        echo ""
    fi
    
    get_stream_status
    echo "🌊 Initial stream status: SUCCEEDED=$STREAMS_SUCCEEDED, NEW=$STREAMS_NEW, FAILED=$STREAMS_FAILED"
    
    get_bootstrap_status
    echo "⚡ Bootstrap: $BOOTSTRAP_PROGRESS% - $BOOTSTRAP_STATUS_TAG ($BOOTSTRAP_STATUS_SUMMARY)"
    
    get_dormant_status
    if [ "$DORMANT" = "1" ]; then
        echo "😴 Tor status: DORMANT (not actively routing)"
    else
        echo "🚀 Tor status: ACTIVE"
    fi
else
    echo "⚠️  Control port $CONTROL_PORT not accessible (traffic monitoring disabled)"
    INITIAL_READ=0
    INITIAL_WRITTEN=0
fi

echo ""
echo "⏱️  Starting timed tests..."
echo "======================================"

ITERATIONS=$((DURATION / INTERVAL))
SUCCESS_TIME=""
PREV_READ=$INITIAL_READ
PREV_WRITTEN=$INITIAL_WRITTEN
PREV_CIRCUITS_BUILT=0
PREV_STREAMS_SUCCEEDED=0

for i in $(seq 1 $ITERATIONS); do
    ELAPSED=$((i * INTERVAL))
    printf "[T+%02ds] Testing... " $ELAPSED
    
    # Use Python to test SOCKS5 (more reliable than curl on macOS)
    RESULT=$(python3 -c "
import socket
import struct
import sys

def test_socks5(proxy_host, proxy_port, dest_host, dest_port, timeout=5):
    try:
        # Connect to SOCKS5 proxy
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        sock.connect((proxy_host, proxy_port))
        
        # SOCKS5 greeting
        sock.sendall(b'\x05\x01\x00')
        
        # Get response
        resp = sock.recv(2)
        if len(resp) < 2 or resp[0] != 5:
            print('SOCKS_ERROR')
            sys.exit(1)
        
        # SOCKS5 connect request
        dest_addr = socket.gethostbyname(dest_host)
        addr_bytes = socket.inet_aton(dest_addr)
        port_bytes = struct.pack('>H', dest_port)
        req = b'\x05\x01\x00\x01' + addr_bytes + port_bytes
        sock.sendall(req)
        
        # Get connect response
        resp = sock.recv(10)
        if len(resp) < 2:
            print('SOCKS_ERROR')
            sys.exit(1)
        
        if resp[1] == 0:
            print('SUCCESS')
            sys.exit(0)
        else:
            print('SOCKS_REFUSED')
            sys.exit(1)
    except socket.timeout:
        print('TIMEOUT')
        sys.exit(1)
    except ConnectionRefusedError:
        print('CONN_REFUSED')
        sys.exit(1)
    except Exception as e:
        print(f'ERROR:{e}')
        sys.exit(1)
    finally:
        try:
            sock.close()
        except:
            pass

test_socks5('$SOCKS_HOST', $SOCKS_PORT, '$TEST_HOST', $TEST_PORT)
" 2>&1)
    
    EXIT_CODE=$?
    
    # Get current traffic stats
    get_traffic_stats
    CURRENT_READ=${TRAFFIC_READ:-0}
    CURRENT_WRITTEN=${TRAFFIC_WRITTEN:-0}
    
    # Get circuit and stream status
    get_circuit_status
    get_stream_status
    
    # Calculate delta
    if [ -n "$PREV_READ" ] && [ "$PREV_READ" != "0" ]; then
        DELTA_READ=$((CURRENT_READ - PREV_READ))
        DELTA_WRITTEN=$((CURRENT_WRITTEN - PREV_WRITTEN))
    else
        DELTA_READ=0
        DELTA_WRITTEN=0
    fi
    
    if [ "$RESULT" = "SUCCESS" ]; then
        echo -n "✅ SUCCESS"
        
        # Show traffic delta if available
        if [ $DELTA_READ -gt 0 ] || [ $DELTA_WRITTEN -gt 0 ]; then
            echo -n " | 📈 Traffic: ↓$(numfmt --to=iec $DELTA_READ 2>/dev/null || echo ${DELTA_READ}B) ↑$(numfmt --to=iec $DELTA_WRITTEN 2>/dev/null || echo ${DELTA_WRITTEN}B)"
        fi
        
        # Show circuit/stream info
        echo -n " | 🔄 Circuits: $CIRCUIT_BUILT built"
        if [ "$STREAMS_SUCCEEDED" != "0" ]; then
            echo -n " | 🌊 Streams: $STREAMS_SUCCEEDED active"
        fi
        
        # Show if circuits/streams changed
        if [ "$CIRCUIT_BUILT" != "$PREV_CIRCUITS_BUILT" ]; then
            DELTA_CIRCUITS=$((CIRCUIT_BUILT - PREV_CIRCUITS_BUILT))
            if [ $DELTA_CIRCUITS -gt 0 ]; then
                echo -n " | 🆕 +$DELTA_CIRCUITS new circuit(s)"
            else
                echo -n " | ⚠️ $DELTA_CIRCUITS circuit(s) closed"
            fi
        fi
        
        echo ""
        
        if [ -z "$SUCCESS_TIME" ]; then
            SUCCESS_TIME=$ELAPSED
            echo ""
            echo "🎉 FIRST SUCCESS AT T+${SUCCESS_TIME}s!"
            echo "SOCKS5 proxy successfully connected to $TEST_HOST"
            echo ""
            echo "Continuing tests to verify stability..."
            echo "--------------------------------------"
        fi
    else
        case "$RESULT" in
            TIMEOUT)
                echo -n "❌ FAILED (timeout)"
                ;;
            CONN_REFUSED)
                echo -n "❌ FAILED (connection refused)"
                ;;
            SOCKS_ERROR)
                echo -n "❌ FAILED (SOCKS protocol error)"
                ;;
            SOCKS_REFUSED)
                echo -n "❌ FAILED (SOCKS server refused connection)"
                ;;
            *)
                echo -n "❌ FAILED ($RESULT)"
                ;;
        esac
        
        # Show traffic even on failure (might indicate background activity)
        if [ $DELTA_READ -gt 0 ] || [ $DELTA_WRITTEN -gt 0 ]; then
            echo -n " | 📈 Traffic: ↓$(numfmt --to=iec $DELTA_READ 2>/dev/null || echo ${DELTA_READ}B) ↑$(numfmt --to=iec $DELTA_WRITTEN 2>/dev/null || echo ${DELTA_WRITTEN}B)"
        fi
        
        # Show what Tor is doing even though SOCKS failed
        echo -n " | 🔄 Circuits: "
        if [ "$CIRCUIT_BUILT" != "0" ]; then
            echo -n "$CIRCUIT_BUILT built"
        fi
        if [ "$CIRCUIT_BUILDING" != "0" ]; then
            echo -n ", $CIRCUIT_BUILDING building"
        fi
        if [ "$CIRCUIT_LAUNCHED" != "0" ]; then
            echo -n ", $CIRCUIT_LAUNCHED launched"
        fi
        
        # Critical: Show if streams are failing
        if [ "$STREAMS_FAILED" != "0" ]; then
            echo -n " | ⚠️ Streams FAILED: $STREAMS_FAILED"
        fi
        
        # Show circuit changes even on failure
        if [ "$CIRCUIT_BUILT" != "$PREV_CIRCUITS_BUILT" ]; then
            DELTA_CIRCUITS=$((CIRCUIT_BUILT - PREV_CIRCUITS_BUILT))
            if [ $DELTA_CIRCUITS -gt 0 ]; then
                echo -n " | 🆕 +$DELTA_CIRCUITS circuit(s)"
            else
                echo -n " | ⚠️ $DELTA_CIRCUITS circuit(s) closed"
            fi
        fi
        
        echo ""
    fi
    
    # Update previous values
    PREV_READ=$CURRENT_READ
    PREV_WRITTEN=$CURRENT_WRITTEN
    PREV_CIRCUITS_BUILT=$CIRCUIT_BUILT
    PREV_STREAMS_SUCCEEDED=$STREAMS_SUCCEEDED
    
    # Don't sleep after last iteration
    if [ $i -lt $ITERATIONS ]; then
        sleep $INTERVAL
    fi
done

echo ""
echo "======================================"
echo "📊 Test Summary"
echo "======================================"

# Get final traffic stats
get_traffic_stats
FINAL_READ=${TRAFFIC_READ:-0}
FINAL_WRITTEN=${TRAFFIC_WRITTEN:-0}

if [ -n "$SUCCESS_TIME" ]; then
    echo "✅ SOCKS proxy became ready at: T+${SUCCESS_TIME}s"
    echo ""
    echo "Analysis:"
    if [ $SUCCESS_TIME -le 5 ]; then
        echo "  🚀 Excellent! SOCKS ready almost immediately (<5s)"
        echo "  This is the expected behavior with proper circuit readiness detection"
    elif [ $SUCCESS_TIME -le 10 ]; then
        echo "  ⚡ Good! SOCKS ready quickly (5-10s)"
        echo "  Minor delay, possibly circuit building time"
    elif [ $SUCCESS_TIME -le 20 ]; then
        echo "  ⏰ Moderate delay (10-20s)"
        echo "  May indicate circuit building or descriptor fetching"
    else
        echo "  🐌 Slow! SOCKS ready after ${SUCCESS_TIME}s (>20s)"
        echo "  This suggests Tor is building additional circuits in background"
        echo "  Your paid circuit might not be used for SOCKS connections"
    fi
else
    echo "❌ SOCKS proxy NEVER became ready during ${DURATION}s test period"
    echo ""
    echo "Troubleshooting:"
    echo "  1. Check if Tor is running: ps aux | grep tor"
    echo "  2. Check Tor logs for errors"
    echo "  3. Verify SOCKS port in torrc matches $SOCKS_PORT"
    echo "  4. Try: curl -v --socks5 $SOCKS_HOST:$SOCKS_PORT $TEST_URL"
fi

# Traffic summary
if [ "$FINAL_READ" != "0" ] && [ "$INITIAL_READ" != "0" ]; then
    TOTAL_READ=$((FINAL_READ - INITIAL_READ))
    TOTAL_WRITTEN=$((FINAL_WRITTEN - INITIAL_WRITTEN))
    
    echo ""
    echo "📊 Traffic Summary:"
    echo "  Initial:  ↓$(numfmt --to=iec $INITIAL_READ 2>/dev/null || echo $INITIAL_READ) ↑$(numfmt --to=iec $INITIAL_WRITTEN 2>/dev/null || echo $INITIAL_WRITTEN)"
    echo "  Final:    ↓$(numfmt --to=iec $FINAL_READ 2>/dev/null || echo $FINAL_READ) ↑$(numfmt --to=iec $FINAL_WRITTEN 2>/dev/null || echo $FINAL_WRITTEN)"
    echo "  Delta:    ↓$(numfmt --to=iec $TOTAL_READ 2>/dev/null || echo $TOTAL_READ) ↑$(numfmt --to=iec $TOTAL_WRITTEN 2>/dev/null || echo $TOTAL_WRITTEN)"
    
    if [ $TOTAL_READ -gt 0 ] || [ $TOTAL_WRITTEN -gt 0 ]; then
        echo "  ✅ Traffic is flowing through Tor!"
        
        # Explain what the traffic might be
        echo ""
        echo "  💡 Traffic sources:"
        echo "     - Directory downloads (consensus, descriptors)"
        echo "     - Circuit building (TLS handshakes, CREATE cells)"
        echo "     - SOCKS connections (if successful)"
        echo "     - Background Tor maintenance"
    else
        echo "  ⚠️  No traffic detected - circuit may not be routing"
    fi
fi

# Final circuit/stream/bootstrap status
echo ""
echo "🔍 Final Tor Status:"
get_circuit_status
echo "  Circuits: BUILT=$CIRCUIT_BUILT, BUILDING=$CIRCUIT_BUILDING, LAUNCHED=$CIRCUIT_LAUNCHED, FAILED=$CIRCUIT_FAILED"

if [ "$CIRCUIT_BUILT" != "0" ] || [ "$CIRCUIT_BUILDING" != "0" ] || [ "$CIRCUIT_LAUNCHED" != "0" ]; then
    echo ""
    echo "  📋 Detailed Circuit List:"
    get_circuit_details
    echo ""
fi

get_stream_status
echo "  Streams: SUCCEEDED=$STREAMS_SUCCEEDED, NEW=$STREAMS_NEW, SENTCONNECT=$STREAMS_SENTCONNECT, FAILED=$STREAMS_FAILED"

get_bootstrap_status
echo "  Bootstrap: $BOOTSTRAP_PROGRESS% - $BOOTSTRAP_STATUS_TAG"

get_dormant_status
if [ "$DORMANT" = "1" ]; then
    echo "  Status: DORMANT (not actively routing)"
else
    echo "  Status: ACTIVE"
fi

# Diagnosis
echo ""
echo "🔬 Diagnosis:"
if [ "$CIRCUIT_BUILT" = "0" ]; then
    echo "  ❌ No circuits built - Tor cannot route traffic yet"
    echo "     → Check bootstrap status above"
    echo "     → Look for circuit build errors in Tor logs"
elif [ -n "$SUCCESS_TIME" ] && [ $SUCCESS_TIME -gt 20 ]; then
    echo "  ⚠️  SOCKS took ${SUCCESS_TIME}s but circuits were built earlier"
    echo "     → Tor likely building its own circuits in background"
    echo "     → Your paid circuit may exist but SOCKS uses different circuits"
    echo ""
    echo "  🔍 Check circuit purposes above:"
    echo "     - PURPOSE=GENERAL circuits can be used for SOCKS"
    echo "     - PURPOSE=CONTROLLER circuits are your paid circuits (if created via EXTENDPAIDCIRCUIT)"
    echo "     - Tor also builds internal circuits for directory fetching"
elif [ -z "$SUCCESS_TIME" ] && [ "$CIRCUIT_BUILT" != "0" ]; then
    echo "  ❌ Circuits exist but SOCKS never worked"
    echo "     → Circuits may have wrong PURPOSE (not GENERAL)"
    echo ""
    echo "  🔍 Looking at circuit purposes:"
    GENERAL_COUNT=$(echo "$CIRCUIT_DETAILS" | grep -c "PURPOSE=GENERAL" || echo "0")
    CONTROLLER_COUNT=$(echo "$CIRCUIT_DETAILS" | grep -c "PURPOSE=CONTROLLER" || echo "0")
    INTERNAL_COUNT=$(echo "$CIRCUIT_DETAILS" | grep -c "PURPOSE=.*INTERNAL" || echo "0")
    
    echo "     - PURPOSE=GENERAL: $GENERAL_COUNT (can be used for SOCKS)"
    echo "     - PURPOSE=CONTROLLER: $CONTROLLER_COUNT (your paid circuits?)"
    echo "     - PURPOSE=*INTERNAL*: $INTERNAL_COUNT (Tor maintenance circuits)"
    echo ""
    
    if [ "$GENERAL_COUNT" = "0" ]; then
        echo "  ⚠️  No GENERAL purpose circuits found!"
        echo "     → This is why SOCKS doesn't work"
        echo "     → Your EXTENDPAIDCIRCUIT should create PURPOSE=GENERAL circuits"
        echo "     → Check el-tor source: control_extendpaidcircuit.c line 134"
    fi
elif [ -n "$SUCCESS_TIME" ]; then
    echo "  ✅ Everything working as expected"
    if [ "$STREAMS_FAILED" != "0" ]; then
        echo "     ⚠️  But $STREAMS_FAILED streams failed - some connection issues"
    fi
    
    # Show which circuits are being used
    GENERAL_COUNT=$(echo "$CIRCUIT_DETAILS" | grep -c "PURPOSE=GENERAL" || echo "0")
    if [ "$GENERAL_COUNT" != "0" ]; then
        echo ""
        echo "  💡 SOCKS is using one of the $GENERAL_COUNT GENERAL purpose circuit(s)"
    fi
fi

echo ""
echo "�💡 Tips:"
echo "  - Run this IMMEDIATELY after seeing 'Circuit is BUILT' message"
echo "  - Compare timing with manual 30-second wait"
echo "  - Check circuit status: echo -e 'AUTHENTICATE \"$CONTROL_PASSWORD\"\\r\\nGETINFO circuit-status\\r\\n' | nc 127.0.0.1 $CONTROL_PORT"
echo "  - Traffic deltas show when data is actually flowing through circuits"
echo ""
