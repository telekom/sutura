''
with subtest("discovery only: roomy reference and combine-only bounded refusal"):
    config = copy.deepcopy(get("configmap/sutura-config"))
    # Keep the exact base bytes; only the environment topology is erased.
    config["metadata"] = {"name": "reference-config"}
    config["data"]["development.yaml"] = json.dumps({"server": {"nodes": None}})
    apply(config)
    assert get("configmap/reference-config")["data"]["base.yaml"] == get("configmap/sutura-config")["data"]["base.yaml"]
    evidence["config"] = {name: get("configmap/" + name)["data"] for name in ["sutura-config", "reference-config"]}

    def calibration_request(mib, label, record):
        bounded = reference(mib * 1024 * 1024)
        before = identity(bounded)
        ceiling_env = [e["value"] for e in bounded["spec"]["containers"][0]["env"] if e["name"] == CEILING_KEY]
        status, reply, _ = finish_query(start_query(label, question(), bounded["status"]["podIP"], correlation=label))
        log = machine.succeed("kubectl logs pod/reference --tail=-1")
        after = identity(get("pod/reference"))
        record.update({"ceiling": mib * 1024 * 1024, "status": status, "reply": reply,
                       "pod": before, "after": after, "ceiling_env": ceiling_env,
                       "correlation": label, "log": log})
        assert after == before and before["ready"] and before["restarts"] == [0], record
        assert ceiling_env == [str(record["ceiling"])], record
        boundaries = []
        for line in log.splitlines():
            if "shuffle reservation refused:" in line:
                print(line)
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if (isinstance(event, dict) and event.get("target") == "sutura_app::federated" and
                    event.get("msg") == "[REQUEST - EVENT] federated execution reached the result boundary"):
                boundaries.append(event)
        record["boundaries"] = boundaries
        assert len(boundaries) == 1, record
        event = boundaries[0]
        assert (event.get("correlation"), event.get("method"), event.get("route"),
                event.get("metric"), event.get("grain")) == (label, "POST", "/v1/query", "total_large", "month"), record
        record["phase"] = event.get("phase")
        return status, reply, record["phase"]

    evidence["roomy"] = {}
    status, reply, phase = calibration_request(512, "calibration-roomy", evidence["roomy"])
    assert status == 200 and phase == "answer", evidence["roomy"]
    expected = exact(reply)
    runtime_images = evidence["roomy"]["pod"]["images"]
    evidence["roomy"]["answer"] = expected
    trials = []
    evidence["calibration"] = trials
    # Refine the bracket and use the highest observed combine refusal as the common bound.
    ceiling, upper, lower, mib = None, None, None, 64
    for attempt in range(7):
        assert 4 <= mib <= 64 and all(t["ceiling"] != mib * 1024 * 1024 for t in trials), trials
        trial = {}
        trials.append(trial)
        status, reply, phase = calibration_request(mib, "calibration-" + str(attempt), trial)
        assert trial["pod"]["images"] == runtime_images, trial
        if status == 200:
            assert phase == "answer" and exact(reply) == expected, trial
            upper = mib
        else:
            assert status == 422 and reply.get("reason", {}).get("code") == "resources_exhausted", trial
            assert str(trial["ceiling"]) in reply["reason"]["detail"], trial
            assert phase in {"leg", "combine"}, trial
            if phase == "combine":
                # execute() reaches combine only after every exact leg has completed successfully.
                ceiling = trial["ceiling"] if ceiling is None else max(ceiling, trial["ceiling"])
            lower = mib
            if upper is None:
                break
        candidate = (lower + upper) // 2 if lower is not None else max(4, mib // 2)
        if candidate == mib or candidate in {lower, upper}:
            break
        mib = candidate
    assert ceiling is not None, (
        "no combine-only ceiling observed",
        [(trial["ceiling"], trial["status"], trial["phase"]) for trial in trials],
    )
    assert identity(get("pod/reference"))["restarts"] == [0]
    assert upper is not None
    evidence["ceiling"] = ceiling
    print("worker-pool calibration (ceiling, phase):", [(t["ceiling"], t["phase"]) for t in trials])
    machine.succeed("kubectl delete pod reference --wait=true")
    set_ceiling("sutura", ceiling)
    set_ceiling("sutura-workers", ceiling)
    serving = one(SERVING)
''
