#!/usr/bin/env bash
#
# The ONE CPU-busy sampler for this repository's utilisation telemetry.
#
# Both composite actions that report host utilisation - `.github/actions/measure-host` and
# `.github/actions/host-probe` - source this file and call `cpu_busy`. A single implementation is
# the whole cure for how the two came to disagree (issue #564): each used to carry its OWN
# /proc/stat arithmetic, and they computed DIFFERENT quantities. measure-host reported the
# *user-mode share of consumed CPU time* (high while the machine was nearly idle) while host-probe
# reported true busy, and a conclusion drawn from the user-share number was mislabelled as
# utilisation. With one sourced function there is no second copy to drift.
#
# Decision, stated because it changes the answer: IOWAIT COUNTS AS NOT-BUSY. An interval that
# spends its time blocked on I/O is not using the CPU, so it is treated like idle - the same choice
# host-probe always made, so the two actions agree by construction.
#
# Usage: cpu_busy <previous_cpu_line> <current_cpu_line>
#
# Each line is the aggregate `/proc/stat` line verbatim, including its leading label:
#
#     cpu user nice system idle iowait irq softirq steal guest guest_nice
#
# Prints the percentage of wall time that was BUSY between the two samples, one decimal, or
# nothing when the delta is degenerate (zero or negative).
# probe(#639 slice I): touches nix/** to move the store-cache digest, measured then reverted.
cpu_busy() {
    local -r prev_line="$1" cur_line="$2"
    awk -v prev="$prev_line" -v cur="$cur_line" '
        BEGIN {
            n = split(prev, p, " ")
            m = split(cur, c, " ")
            pt = 0; ct = 0; pidle = 0; cidle = 0
            for (i = 2; i <= n; i++) { pt += p[i]; if (i == 5 || i == 6) pidle += p[i] }
            for (i = 2; i <= m; i++) { ct += c[i]; if (i == 5 || i == 6) cidle += c[i] }
            dt = ct - pt; di = cidle - pidle
            if (dt > 0) printf "%.1f\n", 100 * (dt - di) / dt
        }
    '
}
