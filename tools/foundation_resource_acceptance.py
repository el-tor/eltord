#!/usr/bin/env python3
"""Prepare/run a finite native bounded-free admission and recovery experiment.

Preparation writes new private fixture files only. Neither mode starts, stops,
reconfigures, builds or pays anything. An operator deploys the lower relay cap
and launches the newly generated dedicated clients before invoking run.
"""
import argparse
import concurrent.futures
import hashlib
import json
import math
import os
from pathlib import Path
import re
import secrets
import shlex
import socket
import stat
import threading
import time

MAX_REPLY = 65536
ECHO_PREFIX = b"foundation-resource-acceptance:"
ECHO_BYTES = len(ECHO_PREFIX) + 16
SCHEMA = {"FORMAT": 1, "STATE": 2, "FREE": 4, "LINK_BYTES": 2,
          "ADMISSION": 5, "TRAFFIC": 5, "PROTOCOL": 5, "WORK": 8,
          "OPERATIONS": 2}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def private_write(path, value):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as output:
        output.write(value)


def read_cookie(path):
    info = path.stat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
            and not info.st_mode & 0o077 and info.st_size == 32,
            "A private owner-only native control cookie is required")
    return path.read_bytes().hex()


class Control:
    def __init__(self, directory, cookie="control_auth_cookie", deadline=None):
        self.deadline = deadline
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.settimeout(2)
        self.socket.connect(str(directory / "control"))
        self.file = self.socket.makefile("rwb", buffering=0)
        require(self.command("AUTHENTICATE " + read_cookie(directory / cookie))
                == ["250 OK"], "Native control authentication failed")

    def close(self):
        self.file.close()
        self.socket.close()

    def command(self, command):
        require("\r" not in command and "\n" not in command,
                "Control commands must be single-line")
        if self.deadline is not None:
            remaining = self.deadline - time.monotonic()
            require(remaining > 0, "Finite experiment deadline elapsed")
            self.socket.settimeout(min(2, remaining))
        self.file.write((command + "\r\n").encode("ascii"))
        lines, size = [], 0
        while True:
            require(self.deadline is None or time.monotonic() < self.deadline,
                    "Finite experiment deadline elapsed")
            raw = self.file.readline(MAX_REPLY + 1)
            size += len(raw)
            require(raw.endswith(b"\r\n") and size <= MAX_REPLY,
                    "Control response framing/size failed")
            line = raw[:-2].decode("ascii")
            require(re.fullmatch(r"[245][0-9]{2}[-+ ].*", line) is not None,
                    "Unexpected native control response")
            lines.append(line)
            if line[3] == "+":
                while True:
                    require(self.deadline is None or time.monotonic() < self.deadline,
                            "Finite experiment deadline elapsed")
                    raw = self.file.readline(MAX_REPLY + 1)
                    size += len(raw)
                    require(raw.endswith(b"\r\n") and size <= MAX_REPLY,
                            "Control data response exceeded its bound")
                    value = raw[:-2].decode("ascii")
                    if value == ".":
                        break
                    lines.append(value[1:] if value.startswith("..") else value)
            elif line[3] == " ":
                return lines

    def info(self, key):
        rows = self.command("GETINFO " + key)
        require(rows[-1] == "250 OK", "Native GETINFO failed")
        if rows[0] == "250+" + key + "=":
            return rows[1:-1]
        prefix = "250-" + key + "="
        require(len(rows) == 2 and rows[0].startswith(prefix),
                "Unexpected GETINFO shape")
        return [rows[0][len(prefix):]]

    def config(self, key):
        rows = self.command("GETCONF " + key)
        prefix = "250 " + key + "="
        require(len(rows) == 1 and rows[0].startswith(prefix),
                "Required immutable native option unavailable")
        return rows[0][len(prefix):]

    def build(self, path):
        # One dispatch only. A lost reply is uncertain, never retransmitted.
        rows = self.command("EXTENDCIRCUIT 0 " + ",".join("$" + fp for fp in path))
        require(len(rows) == 1 and re.fullmatch(r"250 EXTENDED [1-9][0-9]*", rows[0]),
                "Native ordinary constructor did not return a receipt")
        return rows[0].split()[-1]

    def circuits(self):
        return {fields[0]: fields[1] for line in self.info("circuit-status")
                if len(fields := line.split()) >= 2}

    def drop(self, circuit):
        rows = self.command("CLOSECIRCUIT " + circuit)
        require(rows == ["250 OK"] or rows[0].startswith("552 "),
                "Could not close an owned probe circuit")


def snapshot(control):
    rows = control.info("eltor/resource-status")
    result = {}
    require(sum(len(row) + 2 for row in rows) <= 1024,
            "Aggregate snapshot exceeded its fixed bound")
    for row in rows:
        fields = row.split()
        key = fields.pop(0)
        require(key in SCHEMA and key not in result and len(fields) == SCHEMA[key]
                and all(re.fullmatch(r"0|[1-9][0-9]*", v) for v in fields),
                "Aggregate resource snapshot schema mismatch")
        result[key] = [int(v) for v in fields]
        require(all(v <= 2**64 - 1 for v in result[key]), "Resource counter overflow")
    require(result.keys() == SCHEMA.keys() and result["FORMAT"] == [1],
            "Incomplete resource snapshot")
    require(result["STATE"] == [1, 0], "Native resource owner unavailable/exhausted")
    for key, used, cap in [("FREE", 0, 1), ("FREE", 2, 3),
                          ("PROTOCOL", 0, 1), ("PROTOCOL", 2, 3),
                          ("WORK", 0, 1), ("WORK", 2, 3), ("OPERATIONS", 0, 1)]:
        require(result[key][used] <= result[key][cap], "Observed resource ceiling violation")
    return result


def prove_base(control, circuit, hops):
    rows = control.command("ELTOR_CIRCUIT_BIND 1 " + circuit)
    fields = rows[0].split()
    require(len(rows) == 1 and len(fields) == 7
            and fields[:3] == ["250", "ELTOR_CIRCUIT", "1"]
            and int(fields[5]) == hops, "Admitted base circuit binding failed")
    for hop in range(1, hops + 1):
        rows = control.command(f"ELTOR_INITIAL_REGISTRATION_GET 1 {fields[3]} {fields[4]} {hop}")
        require(rows == ["551 UNSUPPORTED_PROFILE"],
                "Probe unexpectedly has financial profile/authority")


def wait_built(control, circuit, deadline):
    while time.monotonic() < deadline:
        state = control.circuits().get(circuit)
        if state == "BUILT":
            return True
        if state is None or state in ("CLOSED", "FAILED"):
            return False
        time.sleep(0.1)
    return False


class Echo:
    def __init__(self):
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(4)
        self.listener.settimeout(0.2)
        self.port = self.listener.getsockname()[1]
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self):
        while not self.stopped.is_set():
            try:
                client, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            with client:
                client.settimeout(2)
                try:
                    data = read_exact(client, ECHO_BYTES)
                    client.sendall(data)
                except (OSError, RuntimeError):
                    pass

    def close(self):
        self.stopped.set()
        self.listener.close()
        self.thread.join(1)


def read_exact(sock, size):
    result = b""
    while len(result) < size:
        data = sock.recv(size - len(result))
        require(bool(data), "Probe transport ended early")
        result += data
    return result


def open_owned_stream(control, circuit, port):
    listeners = shlex.split(" ".join(control.info("net/listeners/socks")))
    require(len(listeners) == 1 and re.fullmatch(r"127\.0\.0\.1:[0-9]+", listeners[0]),
            "Dedicated origin must have exactly one loopback SOCKS listener")
    sock = socket.create_connection(("127.0.0.1", int(listeners[0].split(":")[1])), timeout=5)
    try:
        sock.sendall(b"\x05\x01\x00")
        require(read_exact(sock, 2) == b"\x05\x00", "Native SOCKS negotiation failed")
        sock.sendall(b"\x05\x01\x00\x01\x7f\x00\x00\x01" + port.to_bytes(2, "big"))
        stream, deadline = None, time.monotonic() + 5
        while time.monotonic() < deadline:
            matches = [f for row in control.info("stream-status")
                       if len(f := shlex.split(row)) >= 4 and f[1] == "NEW"
                       and f[3] == f"127.0.0.1:{port}"]
            if len(matches) == 1:
                stream = matches[0][0]
                break
            time.sleep(0.05)
        require(stream is not None, "Dedicated controlled stream was not observed")
        require(control.command(f"ATTACHSTREAM {stream} {circuit}") == ["250 OK"],
                "Could not attach honest stream to its admitted base circuit")
        header = read_exact(sock, 4)
        require(header[:3] == b"\x05\x00\x00", "Bounded-free SOCKS CONNECT failed")
        if header[3] == 1:
            read_exact(sock, 6)
        elif header[3] == 4:
            read_exact(sock, 18)
        else:
            require(header[3] == 3, "Unexpected SOCKS address family")
            read_exact(sock, read_exact(sock, 1)[0] + 2)
        return sock, stream
    except Exception:
        sock.close()
        raise


def owned_stream_succeeded(control, stream, circuit):
    return any(len(f := shlex.split(row)) >= 3 and f[:3] == [stream, "SUCCEEDED", circuit]
               for row in control.info("stream-status"))


def echo_probe(control, circuit, sink):
    started = time.monotonic()
    sock, stream = open_owned_stream(control, circuit, sink.port)
    with sock:
        payload = ECHO_PREFIX + secrets.token_bytes(16)
        sock.sendall(payload)
        require(read_exact(sock, len(payload)) == payload, "Native free echo payload mismatch")
        require(owned_stream_succeeded(control, stream, circuit),
                "Honest stream used a different circuit")
    return round((time.monotonic() - started) * 1000, 3)


def scenario_limits(scenario, max_circuits):
    limits = {"ElTorFoundationMaxCircuits": max_circuits}
    if scenario == "traffic":
        limits.update(ElTorFoundationByteRate=2053,
                      ElTorFoundationByteBurst=16424,
                      ElTorFoundationConnByteRate=8192,
                      ElTorFoundationConnByteBurst=32768,
                      ElTorFoundationQueueBytes=65536)
    elif scenario == "setup":
        limits.update(ElTorFoundationCryptoRate=1,
                      ElTorFoundationCryptoBurst=4,
                      ElTorFoundationMaxRunning=1,
                      ElTorFoundationMaxWaiting=2)
    return limits


LOAD_BYTES = 48 * 1024
LOAD_PAYLOAD = (b"bounded-native-free-traffic:" *
                (1 + LOAD_BYTES // len(b"bounded-native-free-traffic:")))[:LOAD_BYTES]


class TrafficSink:
    """At most eight local transfers with fixed 48 KiB payloads; no public IO."""
    def __init__(self):
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(8)
        self.listener.settimeout(0.2)
        self.port = self.listener.getsockname()[1]
        self.stopped = threading.Event()
        self.clients = []
        self.upload_received = 0
        self.download_offered = 0
        self.lock = threading.Lock()
        self.pool = concurrent.futures.ThreadPoolExecutor(max_workers=8)
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self):
        accepted = 0
        while not self.stopped.is_set() and accepted < 8:
            try:
                client, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            accepted += 1
            with self.lock:
                self.clients.append(client)
            self.pool.submit(self.transfer, client)

    def transfer(self, client):
        try:
            client.settimeout(8)
            transfer_deadline = time.monotonic() + 8
            operation = read_exact(client, 1)
            if operation == b"U":
                received = bytearray()
                while len(received) < LOAD_BYTES:
                    remaining = transfer_deadline - time.monotonic()
                    require(remaining > 0, "Pressure sink deadline elapsed")
                    client.settimeout(remaining)
                    chunk = client.recv(min(4096, LOAD_BYTES - len(received)))
                    require(bool(chunk), "Pressure upload ended early")
                    received.extend(chunk)
                    with self.lock:
                        self.upload_received += len(chunk)
                client.sendall(hashlib.sha256(received).digest())
            elif operation == b"D":
                with self.lock:
                    self.download_offered += LOAD_BYTES
                client.sendall(LOAD_PAYLOAD)
        except (OSError, RuntimeError):
            pass
        finally:
            client.close()

    def close(self):
        self.stopped.set()
        self.listener.close()
        with self.lock:
            for client in self.clients:
                try:
                    client.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                client.close()
        self.thread.join(1)
        self.pool.shutdown(wait=True)


def traffic_transfer(control, circuit, sink, direction):
    # One stream on one owned receipt; no replacement circuit or retry.
    result = {"attached": False, "checksum_complete": False,
              "checksum_mismatch": False,
              "payload_offered": 0, "payload_received": 0}
    try:
        sock, stream = open_owned_stream(control, circuit, sink.port)
        with sock:
            sock.settimeout(8)
            transfer_deadline = time.monotonic() + 8
            require(owned_stream_succeeded(control, stream, circuit),
                    "Traffic stream was not attached to its owned circuit")
            result["attached"] = True
            remaining = transfer_deadline - time.monotonic()
            require(remaining > 0, "Pressure transfer deadline elapsed")
            sock.settimeout(remaining)
            if direction == "upload":
                result["payload_offered"] = LOAD_BYTES
                sock.sendall(b"U" + LOAD_PAYLOAD)
                remaining = transfer_deadline - time.monotonic()
                require(remaining > 0, "Pressure transfer deadline elapsed")
                sock.settimeout(remaining)
                result["checksum_mismatch"] = (read_exact(sock, 32) !=
                                                hashlib.sha256(LOAD_PAYLOAD).digest())
                require(not result["checksum_mismatch"], "Uploaded pressure data checksum mismatch")
            else:
                sock.sendall(b"D")
                received = bytearray()
                while len(received) < LOAD_BYTES:
                    remaining = transfer_deadline - time.monotonic()
                    require(remaining > 0, "Pressure transfer deadline elapsed")
                    sock.settimeout(remaining)
                    chunk = sock.recv(min(4096, LOAD_BYTES - len(received)))
                    require(bool(chunk), "Pressure download ended early")
                    received.extend(chunk)
                    result["payload_received"] += len(chunk)
                result["checksum_mismatch"] = received != LOAD_PAYLOAD
                require(not result["checksum_mismatch"], "Downloaded pressure data checksum mismatch")
        result["checksum_complete"] = True
    except (OSError, RuntimeError):
        pass
    return result


def close_stress(controls, owned):
    for index in range(1, len(controls)):
        for circuit in owned[index]:
            controls[index].drop(circuit)
        owned[index].clear()


def recover_honest(manifest, controls, owned, sink, latencies, deadline):
    require(time.monotonic() + 9 < deadline, "No time remains for honest recovery")
    # Ordinary clock refill; no owner reset, identity replacement, or relaunch.
    time.sleep(8)
    recovered = controls[0].build(manifest["path"])
    owned[0].append(recovered)
    require(wait_built(controls[0], recovered, min(deadline, time.monotonic() + 15)),
            "Fresh honest admission failed after normal refill")
    prove_base(controls[0], recovered, len(manifest["path"]))
    latencies.append(echo_probe(controls[0], recovered, sink))


def traffic_workload(manifest, controls, owned, target, sink, samples,
                     latencies, summary, deadline):
    require(3 <= len(controls) <= 4 and manifest["target"].endswith("/004r"),
            "Traffic fixture requires two/three stress origins and the approved loopback exit")
    phases = []
    summary["traffic_phases"] = phases
    for direction in ("upload", "download"):
        summary["stage"] = "traffic-" + direction + "-build"
        for index in range(1, len(controls)):
            # Building also spends real bytes. Space these admissions before
            # deliberately exhausting the much smaller local traffic bucket.
            time.sleep(3)
            circuit = controls[index].build(manifest["path"])
            owned[index].append(circuit)
            require(wait_built(controls[index], circuit, time.monotonic() + 15),
                    "Pressure circuit failed before the traffic phase")
            prove_base(controls[index], circuit, len(manifest["path"]))
        time.sleep(8)
        source = TrafficSink()
        try:
            summary["stage"] = "traffic-" + direction
            started = time.monotonic()
            before = snapshot(target)
            first_sample = len(samples)
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                results = list(pool.map(lambda index: traffic_transfer(
                    controls[index], owned[index][-1], source, direction),
                    range(1, len(controls))))
            source.close()  # Join bounded sink workers before reading their counters.
            time.sleep(0.3)
            after = snapshot(target)
            elapsed = time.monotonic() - started
            sent = after["LINK_BYTES"][1] - before["LINK_BYTES"][1]
            received = after["LINK_BYTES"][0] - before["LINK_BYTES"][0]
            # Snapshot RTT is included in this conservative monotonic bracket.
            # Already received excess cannot be undone; only outbound debits have
            # a strict process token envelope.
            require(sent <= before["TRAFFIC"][2] + math.ceil(before["TRAFFIC"][1] * elapsed),
                    "Observed outbound traffic exceeded the configured token envelope")
            require(sent > 0 and received > 0, "Both serialized-link traffic directions were not observed")
            completed = sum(r["checksum_complete"] for r in results)
            attached = sum(r["attached"] for r in results)
            payload_seen = (source.upload_received if direction == "upload" else
                            sum(r["payload_received"] for r in results))
            require(not any(r["checksum_mismatch"] for r in results),
                    "Complete pressure payload failed checksum validation")
            require(attached > 0 and payload_seen > 0,
                    "No pressure payload crossed an explicitly owned stream")
            require(completed < len(results), "The finite load did not exercise byte pressure")
            native_closed = sum(controls[index].circuits().get(owned[index][-1]) != "BUILT"
                                for index in range(1, len(controls)))
            require(native_closed > 0,
                    "Byte pressure did not produce an observed native circuit closure")
            phase_samples = samples[first_sample:]
            queued = max((s["FREE"][2] for s in phase_samples), default=0)
            phases.append({"direction": direction, "payload_bytes_per_stream": LOAD_BYTES,
                           "streams": len(results), "attached_streams": attached,
                           "checksum_complete": completed,
                           "pressure_failures": len(results) - completed,
                           "actual_payload_received_bytes": payload_seen,
                           "payload_offered_bytes": (sum(r["payload_offered"] for r in results)
                               if direction == "upload" else source.download_offered),
                           "native_closed_circuits": native_closed,
                           "serialized_received_bytes": received, "serialized_sent_bytes": sent,
                           "elapsed_ms": round(elapsed * 1000), "sampled_queued_bytes": queued})
        finally:
            source.close()
        close_stress(controls, owned)
        summary["stage"] = "traffic-" + direction + "-recovery"
        recover_honest(manifest, controls, owned, sink, latencies, deadline)
    require(any(p["sampled_queued_bytes"] > 0 for p in phases),
            "The finite traffic run did not observe bounded native queue pressure")
    summary.update(passed=True, stage="complete", traffic_phases=phases,
                   honest_echoes=len(latencies), honest_echo_ms=latencies,
                   recovery_after_each_pressure_phase=True)


def setup_workload(manifest, controls, owned, target, sink,
                   latencies, summary, deadline):
    require(3 <= len(controls) <= 4 and manifest["per_origin"] <= 8,
            "Setup fixture is limited to three stress origins/eight requests each")
    summary["stage"] = "setup-storm"
    time.sleep(4)  # Restore the tiny attempt burst after honest construction.
    started = time.monotonic()
    before = snapshot(target)

    def storm(index):
        for _ in range(manifest["per_origin"]):
            owned[index].append(controls[index].build(manifest["path"]))
        built = set()
        until = min(deadline, time.monotonic() + 5)
        while time.monotonic() < until:
            current = controls[index].circuits()
            built.update(c for c in owned[index] if current.get(c) == "BUILT")
            if all(c in built or c not in current for c in owned[index]):
                break
            time.sleep(0.1)
        return len(built), len(owned[index]) - len(built)

    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        results = list(pool.map(storm, range(1, len(controls))))
    after = snapshot(target)
    elapsed = time.monotonic() - started
    attempts = after["WORK"][4] - before["WORK"][4]
    summary.update(attempted_setup_operations=attempts,
                   setup_window_ms=round(elapsed * 1000),
                   stress_built=sum(v[0] for v in results),
                   stress_refused_or_closed=sum(v[1] for v in results))
    require(attempts <= before["WORK"][7] + math.ceil(before["WORK"][6] * elapsed),
            "Observed setup attempts exceeded the configured token envelope")
    require(attempts > 0 and sum(v[1] for v in results) > 0,
            "The finite setup storm did not produce attempted work and refusal")
    # CPU pressure alone must not revoke already admitted tiny honest traffic.
    latencies.append(echo_probe(controls[0], owned[0][0], sink))
    close_stress(controls, owned)
    summary["stage"] = "setup-recovery"
    recover_honest(manifest, controls, owned, sink, latencies, deadline)
    summary.update(passed=True, stage="complete", stress_built=sum(v[0] for v in results),
                   stress_refused_or_closed=sum(v[1] for v in results),
                   attempted_setup_operations=attempts,
                   setup_window_ms=round(elapsed * 1000),
                   honest_echoes=len(latencies), honest_echo_ms=latencies,
                   existing_honest_progress=True, new_honest_recovery=True)


def admission_workload(manifest, controls, owned, target, sink, samples,
                       monitor_errors, latencies, summary):
    summary["stage"] = "saturation"

    def stress(index):
        control = controls[index]
        for attempt in range(manifest["per_origin"]):
            # Three stress origins at <=1 build/s each remain below the
            # shared downstream connection's four-admission/s profile. This
            # scenario targets live state capacity, not setup-rate pressure.
            if attempt:
                time.sleep(1)
            owned[index].append(control.build(manifest["path"]))
        deadline = time.monotonic() + 15
        built = set()
        while time.monotonic() < deadline:
            status = control.circuits()
            built.update(c for c in owned[index] if status.get(c) == "BUILT")
            if all(c in built or c not in status for c in owned[index]):
                break
            time.sleep(0.1)
        if built:
            prove_base(control, next(iter(built)), len(manifest["path"]))
        return len(built), len(owned[index]) - len(built)
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(controls) - 1) as pool:
        results = list(pool.map(stress, range(1, len(controls))))
    summary["stress_built"] = sum(v[0] for v in results)
    summary["stress_refused_or_closed"] = sum(v[1] for v in results)
    require(not monitor_errors and any(s["FREE"][0] == manifest["max_circuits"]
                                       for s in samples), "The actual shared circuit cap was not saturated")
    full = snapshot(target)
    require(full["FREE"][0] == manifest["max_circuits"],
            "Target cap did not remain occupied for cross-origin refusal")
    refused = controls[0].build(manifest["path"]); owned[0].append(refused)
    require(not wait_built(controls[0], refused, time.monotonic() + 5),
            "Another origin bypassed the occupied shared circuit cap")
    latencies.append(echo_probe(controls[0], owned[0][0], sink))
    for index in range(1, len(controls)):
        for circuit in owned[index]:
            controls[index].drop(circuit)
        owned[index].clear()
    summary["stage"] = "recovery"
    # Real clock refill only. No reconnect/reset/config change or replay-row
    # deletion. New honest admission follows bounded actual teardown.
    cooldown = 1 + math.ceil((len(controls) - 1) * manifest["per_origin"]
                             / full["ADMISSION"][1])
    require(cooldown <= 30, "Configured refill exceeds the finite test window")
    time.sleep(max(4, cooldown))
    recovered = controls[0].build(manifest["path"]); owned[0].append(recovered)
    require(wait_built(controls[0], recovered, time.monotonic() + 15),
            "Honest admission did not recover after bounded teardown/refill")
    prove_base(controls[0], recovered, len(manifest["path"]))
    latencies.append(echo_probe(controls[0], recovered, sink))
    require(not monitor_errors, "Aggregate observation failed")
    summary.update(passed=True, stage="complete", honest_echoes=len(latencies),
                   honest_echo_ms=latencies, cross_origin_full_cap_refusal=True)


def prepare(args):
    require(args.scenario == "admission" or (args.origins <= 4 and
            (args.scenario != "setup" or args.per_origin <= 8)),
            "Traffic/setup use at most four origins; setup permits eight requests each")
    require(3 <= args.origins <= 8 and 1 <= args.per_origin <= 24
            and (args.origins - 1) * args.per_origin <= 128
            and 2 <= args.max_circuits <= 64, "Fixture bounds exceeded")
    nodes = args.nodes.resolve()
    names = args.path.split(",")
    require(args.scenario == "admission" or
            (args.scenario == "traffic" and args.target == names[-1]) or
            (args.scenario == "setup" and args.target == names[0]),
            "Traffic targets the exit; setup targets the first hop")
    require(3 <= len(names) <= 16 and all(re.fullmatch(r"[0-9]{3}[acmr]", n) for n in names)
            and args.target in names, "Target must be on the explicit generated-node path")
    fingerprints = [(nodes / name / "fingerprint").read_text().split()[1].upper() for name in names]
    require(len(set(fingerprints)) == len(fingerprints)
            and all(re.fullmatch(r"[A-F0-9]{40}", fp) for fp in fingerprints),
            "Invalid or duplicate relay fingerprint")
    authority = [line for line in args.template.read_text().splitlines()
                 if line.startswith("DirAuthority ")]
    require(1 <= len(authority) <= 16 and all(" 127.0.0.1:" in line for line in authority),
            "Only explicit loopback test authorities may be copied")
    directory = args.directory.resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    origins = []
    for number in range(args.origins):
        node = directory / f"origin-{number}"
        node.mkdir(mode=0o700)
        require(len(str(node / "control").encode()) < 100,
                "Runtime directory is too long for a private UNIX control socket")
        data = node / "data"
        config = "\n".join(authority + [
            "TestingTorNetwork 1", "ElTorFoundation 1", "ClientOnly 1",
            "UseMicrodescriptors 0", "UseEntryGuards 0", "PathsNeededToBuildCircuits 0.67",
            "SocksPort 127.0.0.1:auto", "ControlPort 0", "CookieAuthentication 1",
            "ControlSocket " + json.dumps(str(node / "control")),
            "CookieAuthFile " + json.dumps(str(node / "cookie")),
            "DataDirectory " + json.dumps(str(data)), "RunAsDaemon 0",
            "Log notice file " + str(node / "notice.log"), "SafeLogging 1",
            "__LeaveStreamsUnattached 1", "__DisablePredictedCircuits 1",
            "ClientRejectInternalAddresses 0", "ClientDNSRejectInternalAddresses 0",
            "MaxClientCircuitsPending 16", "LearnCircuitBuildTimeout 0",
            "CircuitBuildTimeout 15", "ExitNodes $" + fingerprints[-1], "StrictNodes 1",
        ]) + "\n"
        private_write(node / "torrc", config)
        origins.append(str(node))
    limits = scenario_limits(args.scenario, args.max_circuits)
    manifest = {"version": 1, "scenario": args.scenario,
                "resource_limits": limits, "target": str(nodes / args.target),
                "path": fingerprints, "origins": origins,
                "per_origin": args.per_origin, "max_circuits": args.max_circuits}
    private_write(directory / "manifest.json", json.dumps(manifest, indent=2) + "\n")
    private_write(directory / "target-overload.fragment",
                  "# Operator deployment only; never applied by this runner.\n" +
                  "".join(f"{name} {value}\n" for name, value in limits.items()))
    maximum_attempts = ((args.origins - 1) * args.per_origin + 3
                        if args.scenario == "admission" else
                        (args.origins - 1) * args.per_origin + 2
                        if args.scenario == "setup" else 2 * (args.origins - 1) + 3)
    print(json.dumps({"prepared_origins": args.origins, "scenario": args.scenario,
                      "maximum_attempts": maximum_attempts,
                      "target_cap": args.max_circuits, "processes_started": 0}))


def run(args):
    directory = args.directory.resolve()
    manifest = json.loads((directory / "manifest.json").read_text())
    require(manifest["version"] == 1 and 3 <= len(manifest["origins"]) <= 8
            and 1 <= manifest["per_origin"] <= 24
            and 2 <= manifest["max_circuits"] <= 64
            and (len(manifest["origins"]) - 1) * manifest["per_origin"] <= 128,
            "Invalid finite fixture manifest")
    require(manifest.get("scenario", "admission") in ("admission", "traffic", "setup"),
            "Unknown finite workload scenario")
    require(3 <= len(manifest["path"]) <= 16
            and len(set(manifest["path"])) == len(manifest["path"])
            and all(re.fullmatch(r"[A-F0-9]{40}", fp) for fp in manifest["path"])
            and all(Path(node).resolve().parent == directory for node in manifest["origins"]),
            "Invalid explicit fixture path or dedicated origin directory")
    # One attempt per fixture, including ambiguous constructors. Preserve every
    # result/config/log; another invocation must not replace or erase history.
    private_write(directory / "run.claim", "claimed\n")
    target, controls, monitor, sink, thread = None, [], None, None, None
    deadline = time.monotonic() + 120
    owned, samples, monitor_errors, latencies = [], [], [], []
    stop = threading.Event()
    summary = {"passed": False, "stage": "preflight", "payment_commands": 0,
               "wallet_calls": 0, "processes_started": 0,
               "binary_sha256": args.binary_sha256, "limitations":
               "Sampled counters, a small finite workload, no production/privacy/paid-settlement proof"}
    try:
        target = Control(Path(manifest["target"]), deadline=deadline)
        target_fingerprint = (Path(manifest["target"]) / "fingerprint").read_text().split()[1].upper()
        require(target_fingerprint in manifest["path"], "Target is not on the selected path")
        scenario = manifest.get("scenario", "admission")
        require(scenario == "admission" or
                (scenario == "traffic" and target_fingerprint == manifest["path"][-1]) or
                (scenario == "setup" and target_fingerprint == manifest["path"][0]),
                "Target role differs from the reviewed workload")
        for name in ("TestingTorNetwork", "ElTorFoundation"):
            require(target.config(name) == "1", "Isolated explicit foundation mode required")
        require(int(target.config("ElTorFoundationMaxCircuits")) == manifest["max_circuits"],
                "Operator must deploy the exact lower target cap before this experiment")
        before = snapshot(target)
        require(before["FREE"][0] < manifest["max_circuits"] - 1,
                "Target lacks baseline headroom for honest admission")
        if scenario == "setup":
            require(before["FREE"][0] + (len(manifest["origins"]) - 1) *
                    manifest["per_origin"] + 1 <= manifest["max_circuits"],
                    "Setup workload needs live-state headroom to distinguish crypto pressure")
        pids = set()
        for node in manifest["origins"]:
            control = Control(Path(node), "cookie", deadline=deadline)
            controls.append(control); owned.append([])
            require(control.config("ElTorFoundation") == "1"
                    and control.config("TestingTorNetwork") == "1"
                    and control.config("__LeaveStreamsUnattached") == "1"
                    and control.config("__DisablePredictedCircuits") == "1",
                    "Origins must be dedicated controlled-stream foundation clients")
            phase = " ".join(control.info("status/bootstrap-phase"))
            progress = re.search(r"(?:^| )PROGRESS=([0-9]+)(?: |$)", phase)
            # Predicted circuits are deliberately disabled. Tor reaches 100%
            # only after our first explicit circuit; full descriptors are checked
            # below before this runner is permitted to dispatch it.
            require(progress is not None and 75 <= int(progress.group(1)) <= 100,
                    "Dedicated origin has not reached descriptor-ready bootstrap")
            pid = int(control.info("process/pid")[0])
            require(pid not in pids, "Origins must be independent native processes")
            pids.add(pid)
            require(hashlib.sha256(Path(f"/proc/{pid}/exe").read_bytes()).hexdigest()
                    == args.binary_sha256, "Actual origin binary provenance mismatch")
            for fp in manifest["path"]:
                descriptor = control.info("desc/id/" + fp)
                require("eltor-free-capacity 1 2 128 600000" in descriptor,
                        "Missing authenticated per-hop free capability")
        target_pid = int(target.info("process/pid")[0])
        require(target_pid not in pids and hashlib.sha256(
            Path(f"/proc/{target_pid}/exe").read_bytes()).hexdigest() == args.binary_sha256,
            "Actual target process/binary provenance mismatch")
        limits = manifest.get("resource_limits", {
            "ElTorFoundationMaxCircuits": manifest["max_circuits"]})
        expected_limits = scenario_limits(manifest.get("scenario", "admission"),
                                          manifest["max_circuits"])
        require(limits == expected_limits, "Fixture lower limits differ from the reviewed scenario")
        for name, value in limits.items():
            require(int(target.config(name)) == value,
                    "Operator must deploy every exact immutable scenario limit")
        summary["scenario"] = manifest.get("scenario", "admission")
        summary["configured_limits"] = limits
        summary["origins"] = len(controls)
        summary["target_cap"] = manifest["max_circuits"]
        summary["baseline"] = before

        monitor = Control(Path(manifest["target"]), deadline=deadline)
        interval = 0.25 if summary["scenario"] == "admission" else 0.1
        summary["sample_interval_ms"] = round(interval * 1000)
        maximum_samples = 512 if summary["scenario"] == "admission" else 1200
        def sample():
            while not stop.is_set() and len(samples) < maximum_samples:
                try:
                    current = snapshot(monitor)
                    if samples:
                        require(current["LINK_BYTES"][0] >= samples[-1]["LINK_BYTES"][0]
                                and current["LINK_BYTES"][1] >= samples[-1]["LINK_BYTES"][1]
                                and current["WORK"][4] >= samples[-1]["WORK"][4],
                                "Counters reset during the experiment")
                    samples.append(current)
                except Exception:
                    monitor_errors.append("Resource observation failed")
                    break
                stop.wait(interval)
        thread = threading.Thread(target=sample, daemon=True)
        thread.start()
        sink = Echo()
        summary["stage"] = "baseline-build"
        honest = controls[0].build(manifest["path"]); owned[0].append(honest)
        require(wait_built(controls[0], honest, time.monotonic() + 15),
                "Baseline honest base circuit failed")
        summary["stage"] = "baseline-bind"
        prove_base(controls[0], honest, len(manifest["path"]))
        require("PROGRESS=100" in " ".join(controls[0].info("status/bootstrap-phase")),
                "Honest origin did not complete bootstrap on its explicit circuit")
        summary["stage"] = "baseline-echo"
        latencies.append(echo_probe(controls[0], honest, sink))
        scenario = manifest.get("scenario", "admission")
        if scenario == "admission":
            admission_workload(manifest, controls, owned, target, sink, samples,
                               monitor_errors, latencies, summary)
        elif scenario == "traffic":
            traffic_workload(manifest, controls, owned, target, sink, samples,
                             latencies, summary, deadline)
        else:
            setup_workload(manifest, controls, owned, target, sink,
                           latencies, summary, deadline)
        summary["after_recovery"] = snapshot(target)
    except Exception:
        summary["passed"] = False
        # Do not print controller replies, opaque handles, private paths, cookies
        # or any arbitrary exception body. The private stage names locate failure.
        summary["failure"] = "Finite acceptance requirement failed; inspect private native fixture logs"
    finally:
        stop.set()
        if monitor:
            if thread:
                thread.join(3)
            monitor.close()
        if monitor_errors:
            summary["passed"] = False
            summary["failure"] = "Aggregate observation failed"
        for index, control in enumerate(controls):
            control.deadline = time.monotonic() + 2
            for circuit in owned[index]:
                try:
                    control.drop(circuit)
                except Exception:
                    summary["cleanup_incomplete"] = True
            control.close()
        if sink:
            sink.close()
        if target:
            target.close()
        summary["samples"] = len(samples)
        if samples:
            summary["sampled_peaks"] = {"free_circuits": max(s["FREE"][0] for s in samples),
                "queued_bytes": max(s["FREE"][2] for s in samples),
                "slots": max(s["PROTOCOL"][0] for s in samples),
                "arena_bytes": max(s["PROTOCOL"][2] for s in samples),
                "running": max(s["WORK"][0] for s in samples),
                "waiting": max(s["WORK"][2] for s in samples)}
        private_write(directory / "result.json", json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, sort_keys=True))
    return 0 if summary["passed"] and not summary.get("cleanup_incomplete") else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prep = commands.add_parser("prepare")
    prep.add_argument("--nodes", type=Path, required=True)
    prep.add_argument("--template", type=Path, required=True)
    prep.add_argument("--directory", type=Path, required=True)
    prep.add_argument("--path", default="006m,007m,004r")
    prep.add_argument("--target", default="006m")
    prep.add_argument("--scenario", choices=("admission", "traffic", "setup"),
                      default="admission")
    prep.add_argument("--origins", type=int, default=4)
    prep.add_argument("--per-origin", type=int, default=16)
    prep.add_argument("--max-circuits", type=int, default=24)
    execute = commands.add_parser("run")
    execute.add_argument("--directory", type=Path, required=True)
    execute.add_argument("--binary-sha256", required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args)
        return 0
    require(re.fullmatch(r"[0-9a-f]{64}", args.binary_sha256), "Expected binary SHA256 is required")
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())
