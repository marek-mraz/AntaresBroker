#!/usr/bin/env python3
"""1 Hz CPU/RSS sampler for the ETSI stack, labelled with the running suite.

Reads /proc/<container pid>/{stat,status} directly rather than shelling out to
`docker stats`: a `docker stats --no-stream` call costs ~1 s of its own (it
samples twice to compute CPU%), which makes a 1 s cadence impossible, and it
reports "0B / 0B" under a nested daemon with no delegated memory cgroup. The
container pid is visible in our own /proc because the daemon is our child.

Emits `ts,iso,container,phase,cpu_pct,rss_mib` — one row per container per
tick, flushed every tick so a SIGKILLed sampler still leaves complete data.
`phase` is read from --phase-file on every tick (the suite runner writes the
suite name there), which is what lets a spike be traced back to the suite that
caused it. Per-TEST attribution is a post-processing join on `ts` against the
Robot output.xml timestamps — see the report step in dev/etsi-pipeline.sh.

`ponytail:` main-pid CPU only (the broker is one process); sum the cgroup if a
role ever forks workers.
"""
import argparse
import os
import subprocess
import sys
import time

CLK_TCK = os.sysconf("SC_CLK_TCK")


def containers():
    """name -> pid for every running antares container (0 = pid unknown)."""
    try:
        out = subprocess.run(
            ["docker", "ps", "--filter", "name=antares", "--format", "{{.Names}}"],
            capture_output=True, text=True, timeout=20,
        ).stdout.split()
    except Exception:
        return {}
    found = {}
    for name in out:
        try:
            pid = subprocess.run(
                ["docker", "inspect", "-f", "{{.State.Pid}}", name],
                capture_output=True, text=True, timeout=20,
            ).stdout.strip()
            found[name] = int(pid)
        except Exception:
            found[name] = 0
    return found


def cpu_ticks(pid):
    """utime+stime in clock ticks, or None if the process is gone."""
    try:
        with open(f"/proc/{pid}/stat") as fh:
            data = fh.read()
        # comm can contain spaces and parens — everything after the LAST ')'
        fields = data[data.rindex(")") + 1:].split()
        return int(fields[11]) + int(fields[12])  # utime, stime (14/15 1-indexed)
    except Exception:
        return None


def rss_mib(pid):
    try:
        with open(f"/proc/{pid}/status") as fh:
            for line in fh:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1]) / 1024
    except Exception:
        pass
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--phase-file", default="")
    ap.add_argument("--interval", type=float, default=1.0)
    args = ap.parse_args()

    procs = containers()
    last_refresh = time.time()
    prev = {}  # name -> (ticks, monotonic)

    with open(args.out, "w", buffering=1) as out:
        out.write("ts,iso,container,phase,cpu_pct,rss_mib\n")
        while True:
            now = time.time()
            mono = time.monotonic()

            # Re-resolve when a container vanished (restart) or every 30 s, so a
            # broker that gets recreated mid-run keeps being sampled.
            if now - last_refresh > 30 or any(
                p == 0 or not os.path.exists(f"/proc/{p}") for p in procs.values()
            ):
                procs = containers()
                last_refresh = now

            phase = ""
            if args.phase_file:
                try:
                    with open(args.phase_file) as fh:
                        phase = fh.read().strip().replace(",", ";")
                except OSError:
                    pass

            iso = time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(now))
            for name, pid in sorted(procs.items()):
                if not pid:
                    continue
                ticks, rss = cpu_ticks(pid), rss_mib(pid)
                if ticks is None and rss is None:
                    continue
                cpu = ""
                if ticks is not None:
                    was = prev.get(name)
                    if was and mono > was[1]:
                        cpu = f"{(ticks - was[0]) / CLK_TCK / (mono - was[1]) * 100:.1f}"
                    prev[name] = (ticks, mono)
                out.write(
                    f"{now:.0f},{iso},{name},{phase},{cpu},"
                    f"{'' if rss is None else f'{rss:.1f}'}\n"
                )

            time.sleep(max(0.0, args.interval - (time.monotonic() - mono)))


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(0)
