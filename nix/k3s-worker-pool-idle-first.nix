''
with subtest("a real busy UID survives one controlled idle-first removal"):
    pool(min=2, max=2, warm=0, idleAfterSeconds=0)
    settled(2)
    pair = worker_pods(2)
    busy, idle = pair
    assert all(ready(p) and identity(p)["restarts"] == [0] for p in pair)
    assert identity(busy)["priority"] == identity(idle)["priority"]
    assert busy["spec"]["nodeName"] != idle["spec"]["nodeName"]
    occupancy = {p["spec"]["nodeName"]: sum(other["spec"]["nodeName"] == p["spec"]["nodeName"]
                 for other in pair) for p in pair}
    assert set(occupancy.values()) == {1}, occupancy
    for target in pair:
        assert gauge(target, "sutura_worker_requests_in_flight") == 0
        patch("pod/" + target["metadata"]["name"], {"metadata": {"annotations": {
            "controller.kubernetes.io/pod-deletion-cost": None}}})
    host, prefix = namespace(busy)
    # Stall only this worker's client flow to the serving node. Operator scrapes and
    # kubelet probes use different tuples and stay live; no metric value is invented.
    gate = f"OUTPUT -d {serving['status']['podIP']} -p tcp --dport 8080 -j DROP"
    watching, gated = False, False
    try:
        # Query discovery sees one ready peer; the operator still sees both by the
        # Deployment selector and scrapes their IPs. No startup is inside the held query.
        patch("pod/" + busy["metadata"]["name"], {"metadata": {"labels": {"fixture-query-peer": "true"}}})
        patch("service/sutura-workers", {"spec": {"selector": {"fixture-query-peer": "true"}}})
        endpoints([busy])
        machine.succeed("kubectl get pods -l " + q(WORKERS) +
                        " --watch --output-watch-events -o json > /tmp/deletions.json 2>/tmp/deletions.stderr </dev/null & echo $! > /tmp/deletions.pid")
        watching = True
        host.succeed(prefix + "iptables -I " + gate)
        gated = True
        request = start_query("busy", question(), address)
        eventually(lambda: gauge(busy, "sutura_worker_requests_in_flight") > 0, 120)
        assert machine.execute("test -f /tmp/busy.exit")[0] != 0, "question ended before the busy witness"
        counts = {p["metadata"]["uid"]: gauge(p, "sutura_worker_requests_in_flight") for p in [busy, idle]}
        assert counts[busy["metadata"]["uid"]] > 0 and counts[idle["metadata"]["uid"]] == 0, counts
        pool(min=1, max=1, idleAfterSeconds=0)
        # Observe the deletion decision while busy, then resume before waiting for the
        # idle pod's drain. The completion budget does not include Kubernetes' grace.
        samples = []
        def deleting_idle():
            current = pods(WORKERS)
            survivor = next((p for p in current if p["metadata"]["uid"] == busy["metadata"]["uid"]), None)
            assert survivor is not None and ready(survivor), current
            samples.append(gauge(busy, "sutura_worker_requests_in_flight"))
            assert samples[-1] > 0, samples
            candidate = next((p for p in current if p["metadata"]["uid"] == idle["metadata"]["uid"]), None)
            return candidate is None or bool(candidate["metadata"].get("deletionTimestamp"))
        eventually(deleting_idle, 60)
        host.succeed(prefix + "iptables -D " + gate)
        gated = False
        status, reply, _ = finish_query(request)
        assert status == 200 and exact(reply) == expected, reply
        def removed():
            current = pods(WORKERS)
            return current if len(current) == 1 and ready(current[0]) else False
        survivors = eventually(removed, 60)
        assert survivors[0]["metadata"]["uid"] == busy["metadata"]["uid"], survivors
        assert samples and all(value > 0 for value in samples), samples
        def deletion_seen():
            try:
                records = watch_records(machine.succeed("cat /tmp/deletions.json"))
            except json.JSONDecodeError:
                return False
            return any(r["type"] == "DELETED" and r["object"]["metadata"]["uid"] == idle["metadata"]["uid"] for r in records)
        eventually(deletion_seen, 30)
        machine.succeed('kill "$(cat /tmp/deletions.pid)"')
        watching = False
        records = watch_records(machine.succeed("cat /tmp/deletions.json"))
        costs = {p["metadata"]["uid"]: [] for p in [busy, idle]}
        for record in records:
            pod = record["object"]
            if not pod["metadata"].get("deletionTimestamp"):
                assert ready(pod) and identity(pod)["restarts"] == [0], pod
            uid = pod["metadata"]["uid"]
            value = pod["metadata"].get("annotations", {}).get("controller.kubernetes.io/pod-deletion-cost")
            if uid in costs and value is not None:
                costs[uid].append(int(value))
        assert any(value > 0 for value in costs[busy["metadata"]["uid"]]), costs
        assert 0 in costs[idle["metadata"]["uid"]], costs
        assert any(r["object"]["metadata"]["uid"] == idle["metadata"]["uid"] and
                   r["object"]["metadata"].get("deletionTimestamp") for r in records), records
        evidence["idle-first"] = {"busy": identity(busy), "idle": identity(idle),
            "before": counts, "busy-samples": samples, "costs": costs, "occupancy": occupancy,
            "survivor": identity(survivors[0]), "watch": records,
            "resumed-answer": exact(reply),
            "busy-until": "idle deletion decision; full deletion observed after the exact answer",
            "limit": "one controlled observation; deletion cost is a preference with a read-to-delete race"}
    finally:
        try:
            if gated:
                host.succeed(prefix + "iptables -D " + gate)
            if watching:
                machine.execute('kill "$(cat /tmp/deletions.pid)"')
        finally:
            patch("service/sutura-workers", {"spec": {"selector": {"fixture-query-peer": None}}})
            patch("pod/" + busy["metadata"]["name"], {"metadata": {"labels": {"fixture-query-peer": None}}})
    endpoints(survivors)
    pool(min=0)
    settled(0)
''
