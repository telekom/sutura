''
with subtest("roomy reference and combine-only refusal at the common ceiling"):
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
    runtime_images = evidence["roomy"]["pod"]["images"]
    evidence["roomy"]["answer"] = exact(reply)
    bounded = {}
    evidence["calibration"] = [bounded]
    # The fixture answered at 11 MiB and reached a combine refusal at 10 MiB.
    status, reply, phase = calibration_request(10, "calibration-bounded", bounded)
    ceiling = bounded["ceiling"]
    assert bounded["pod"]["images"] == runtime_images, bounded
    assert status == 422 and reply.get("reason", {}).get("code") == "resources_exhausted", bounded
    assert str(ceiling) in reply["reason"]["detail"], bounded
    assert phase == "combine", bounded
    assert identity(get("pod/reference"))["restarts"] == [0]
    evidence["ceiling"] = ceiling
    print("worker-pool common ceiling (bytes):", ceiling)
    machine.succeed("kubectl delete pod reference --wait=true")
    set_ceiling("sutura", ceiling)
    set_ceiling("sutura-workers", ceiling)
    serving = one(SERVING)
''
