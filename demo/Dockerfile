# The local chat demo, as demo-only packaging.
#
# TWO STAGES, and the first is the point. The shipped `sutura-serve` image holds ONE executable and
# no shell (`nix/oci.nix` builds it that way on purpose), so it cannot supervise the chat client and
# cannot answer a `CMD-SHELL` probe. The second stage is the chat client's own image with that
# binary, a `python3` probe and the supervisor added - the least that can do both.
#
# NOT a release artifact. `just demo` builds this locally from the two images below; a release
# publishes the server image on its own. `demo/start.sh` loads the server image first and passes the
# tag in, because a compose service may not build here (see `no_service_in_the_tier_is_built_here`).
ARG SUTURA_SERVE_IMAGE=sutura-serve:latest
ARG SUTURA_DEMO_BASE_IMAGE=docker.io/openwebui/open-webui:0.11.3

FROM ${SUTURA_SERVE_IMAGE} AS server

FROM ${SUTURA_DEMO_BASE_IMAGE}

# The shipped server executable, at the path the supervisor below calls. Copied OUT of the image
# rather than rebuilt, so what the demo drives is the same binary a release publishes.
COPY --from=server /bin/sutura-serve /usr/local/bin/sutura-serve

# The demo's own code, and the read-only corpus it serves. COPIED rather than bind-mounted: a host
# bind mount belongs to the machine and every worktree would share it, which the compose tier
# refuses (`no_service_in_the_tier_mounts_a_host_directory`). A copied tree cannot be written back.
COPY demo/run.sh demo/healthcheck.py /usr/local/bin/
COPY examples/single-player /opt/sutura-demo/corpus
RUN chmod 0755 /usr/local/bin/run.sh /usr/local/bin/healthcheck.py

ENV SUTURA_DEMO_CORPUS=/opt/sutura-demo/corpus \
    SUTURA_DEMO_SUTURA_PORT=9000 \
    SUTURA_DEMO_WEBUI_PORT=8080 \
    SUTURA_DEMO_RUN_DIR=/run/sutura-demo

EXPOSE 8080

# Clear the base image's `CMD ["bash", "start.sh"]`: `run.sh` starts that itself, and inheriting the
# CMD would pass it to `run.sh` as arguments.
CMD []

# The base image's own healthcheck probes only the chat client. This one probes the server too, and
# the tool surface the demo exists for - a client that is up while its server is not is unhealthy.
HEALTHCHECK --interval=3s --timeout=5s --start-period=45s --retries=60 \
  CMD ["python3", "/usr/local/bin/healthcheck.py"]

ENTRYPOINT ["/usr/local/bin/run.sh"]
