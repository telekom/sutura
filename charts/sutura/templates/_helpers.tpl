{{/*
Naming, common labels, and the render-time guards that mirror `Settings::refusals` and
`Settings::query_surface_refusals` - `crates/sutura-config/src/settings.rs` - close enough to catch a broken values file before
`kubectl apply` does. These are a chart-side heuristic, not the binary's own check: the
binary's refusal is the one that actually runs, and it sees the fully layered settings tree
this chart cannot evaluate (a Secret's contents, an environment-specific override file).
*/}}

{{- define "sutura.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "sutura.fullname" -}}
{{- printf "%s" .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "sutura.labels" -}}
app.kubernetes.io/name: {{ include "sutura.name" . }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Values.image.tag | default .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "sutura.selectorLabels" -}}
app.kubernetes.io/name: {{ include "sutura.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/*
The bind is always off-host: this chart's Service targets the pod, so `server.host` is
always `0.0.0.0`, never the loopback-only embedded default. `Settings::refusals` therefore
always asks for a credential, a declared TLS termination, and an enabled rate limiter -
this fails the render rather than shipping a Deployment that crash-loops on the first refusal.

The credential is an access token or an inbound identity (`security.inbound.mode`), and `direct`
refuses an access token beside it. Both are read the way the binary layers the two files: the
environment file over `base.yaml`, key by key. A `SUTURA__*` variable in `extraEnv` is not read.

With no inbound mode, `sutura serve` starts `/v1` only for a `single-user` deployment with no
`impersonation-at-source` source (`Settings::query_surface_refusals`). Those two guards come last,
so a values file that breaks an earlier rule still fails at that rule.
*/}}
{{- define "sutura.requireOffHostPosture" -}}
{{- if not .Values.environment -}}
{{ fail "environment is required (SUTURA_ENVIRONMENT) - see values.yaml" }}
{{- end -}}
{{- if not (has .Values.environment (list "development" "test" "production")) -}}
{{ fail (printf "environment is %q: it must be exactly one of development, test, production, spelled as sutura names them (SUTURA_ENVIRONMENT and the config.environment file)" (toString .Values.environment)) }}
{{- end -}}
{{- $base := .Values.config.base | default dict -}}
{{- $env := .Values.config.environment | default dict -}}
{{- $mode := dig "security" "inbound" "mode" (dig "security" "inbound" "mode" "" $base) $env | default "" | toString | trim -}}
{{- $token := or .Values.security.accessToken.secretName (dig "security" "access_token" (dig "security" "access_token" "" $base) $env | default "" | toString) -}}
{{- if not (or $token $mode) -}}
{{ fail "security.accessToken.secretName is required when config.base and config.environment set no security.inbound.mode: this chart's Service makes the pod reachable off-host, and sutura refuses to start there with neither security.access_token nor an inbound identity (Settings::refusals::AccessTokenRequired)" }}
{{- end -}}
{{- if and $token (eq $mode "direct") -}}
{{ fail "security.inbound.mode is `direct` and an access token is set (security.accessToken.secretName, or security.access_token in config): sutura refuses to start with both, because both are read from `authorization: Bearer` (Settings::refusals::DeploymentTokenSharesTheHeader) - remove the access token, or declare `behind-gateway`, whose proof arrives in a header of its own" }}
{{- end -}}
{{- if not .Values.security.metricsToken.secretName -}}
{{ fail "security.metricsToken.secretName is required: this chart's Service makes the pod reachable off-host, and sutura refuses to start with no security.metrics_token there (Settings::refusals::MetricsTokenRequired)" }}
{{- end -}}
{{- $tlsDeclared := or .Values.tls.enabled (ne .Values.security.tlsTermination "") -}}
{{- if not $tlsDeclared -}}
{{ fail "set tls.enabled=true or security.tlsTermination (sidecar|ingress|in-process): sutura refuses to start off-host with security.tls_termination undeclared (Settings::refusals::TlsTerminationUndeclared)" }}
{{- end -}}
{{- if and .Values.tls.enabled (not .Values.tls.secretName) -}}
{{ fail "tls.enabled requires tls.secretName (a kubernetes.io/tls Secret)" }}
{{- end -}}
{{- if not $mode -}}
{{- $identity := dig "security" "identity" (dig "security" "identity" "" $base) $env | default "" | toString | trim -}}
{{- if ne $identity "single-user" -}}
{{ fail "config.base and config.environment set no security.inbound.mode and security.identity is not single-user: sutura serve refuses to start /v1 with no inbound identity unless one user's own credentials run every query (Settings::query_surface_refusals::QuerySurfaceWithoutInboundIdentity) - set security.identity: single-user with a reason in security.single_user_because, or declare security.inbound" }}
{{- end -}}
{{- range $alias, $source := mergeOverwrite (deepCopy (dig "sources" (dict) $base)) (dig "sources" (dict) $env) -}}
{{- if eq (dig "posture" "" $source | toString | trim) "impersonation-at-source" -}}
{{ fail (printf "sources.%s is impersonation-at-source and config.base and config.environment set no security.inbound.mode: sutura serve refuses to start /v1 over a source that runs each query as a verified caller when no inbound identity verifies one (Settings::query_surface_refusals::QuerySurfaceOverAnImpersonatingSource) - declare security.inbound, or give the source another posture" $alias) }}
{{- end -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{/*
`runtime.engineWorkerThreads`, derived from `resources.limits.cpu` when the operator has not
set it explicitly - #149 branch 3. `available_parallelism` (`runtime.engine_worker_threads` in
docs/serving.md) reports the HOST's core count, which over-counts under a CPU quota; this closes that
trap the same way an operator would by hand, by rounding the limit up to a whole thread. An
explicit `runtime.engineWorkerThreads` always wins, and an absent limit leaves the key unset -
the binary's own "as many threads as this machine reports" default, unchanged from before this
template existed.

Accepts a bare core count ("2", "1.5") or a millicpu suffix ("1500m"); rounds UP so a
fractional limit never asks for zero threads, and clamps to `WorkerCount::MAX`
(`crates/sutura-config/src/runtime.rs`) so a limit typo cannot request a thousand threads. A
limit that parses to no cores at all fails the render rather than silently deriving one thread.
*/}}
{{- define "sutura.engineWorkerThreads" -}}
{{- if .Values.runtime.engineWorkerThreads -}}
{{- .Values.runtime.engineWorkerThreads -}}
{{- else if (dig "limits" "cpu" "" .Values.resources) -}}
{{- $cpu := dig "limits" "cpu" "" .Values.resources | toString -}}
{{- $cores := 0.0 -}}
{{- if hasSuffix "m" $cpu -}}
{{- $cores = divf (trimSuffix "m" $cpu | float64) 1000.0 -}}
{{- else -}}
{{- $cores = float64 $cpu -}}
{{- end -}}
{{- if le $cores 0.0 -}}
{{ fail (printf "resources.limits.cpu %q is not a quantity this chart can derive runtime.engineWorkerThreads from - set runtime.engineWorkerThreads explicitly" $cpu) }}
{{- end -}}
{{- min 256 (max 1 (ceil $cores | int)) -}}
{{- end -}}
{{- end -}}

{{/*
`runtime.workingSetMaxBytes`, derived from `resources.limits.memory` when the operator has not
set it explicitly - #149 branch 3, the second trap: `runtime.working_set_max_bytes` in
`docs/serving.md` ships at one provisional gibibyte, unrelated to whatever memory limit the
Deployment actually carries.
An explicit `runtime.workingSetMaxBytes` always wins; an absent limit leaves the key unset.

Reserves a fixed fraction of the limit rather than handing over all of it: `docs/serving.md`'s
`resources_exhausted` refusal (`422`) only comes back if the ceiling trips before the kernel OOM-kills
the container at the cgroup limit, and this process's own RSS (binary, thread stacks, page
cache) is never zero. 0.75 is a chosen headroom, not a measurement - the same status
`WorkingSetCeiling::DEFAULT_BYTES` itself carries - and the escape hatch for a deployment that
has measured its own overhead is the same one above: set `runtime.workingSetMaxBytes` directly.

Accepts the binary (Ki/Mi/Gi/Ti) and decimal (K/M/G/T) suffixes Kubernetes quantities use, or a
bare byte count; an exponential quantity ("1e3") is not supported and is read as a bare number,
which is this template's stated limit rather than a silent one. A quantity that parses to
nothing (`Pi`, a lowercase `k`, a typo) fails the render rather than deriving a zero ceiling.
*/}}
{{- define "sutura.workingSetMaxBytes" -}}
{{- if .Values.runtime.workingSetMaxBytes -}}
{{- .Values.runtime.workingSetMaxBytes -}}
{{- else if (dig "limits" "memory" "" .Values.resources) -}}
{{- $mem := dig "limits" "memory" "" .Values.resources | toString -}}
{{- $bytes := 0.0 -}}
{{- if hasSuffix "Ki" $mem -}}
{{- $bytes = mulf (trimSuffix "Ki" $mem | float64) 1024.0 -}}
{{- else if hasSuffix "Mi" $mem -}}
{{- $bytes = mulf (trimSuffix "Mi" $mem | float64) 1048576.0 -}}
{{- else if hasSuffix "Gi" $mem -}}
{{- $bytes = mulf (trimSuffix "Gi" $mem | float64) 1073741824.0 -}}
{{- else if hasSuffix "Ti" $mem -}}
{{- $bytes = mulf (trimSuffix "Ti" $mem | float64) 1099511627776.0 -}}
{{- else if hasSuffix "K" $mem -}}
{{- $bytes = mulf (trimSuffix "K" $mem | float64) 1000.0 -}}
{{- else if hasSuffix "M" $mem -}}
{{- $bytes = mulf (trimSuffix "M" $mem | float64) 1000000.0 -}}
{{- else if hasSuffix "G" $mem -}}
{{- $bytes = mulf (trimSuffix "G" $mem | float64) 1000000000.0 -}}
{{- else if hasSuffix "T" $mem -}}
{{- $bytes = mulf (trimSuffix "T" $mem | float64) 1000000000000.0 -}}
{{- else -}}
{{- $bytes = float64 $mem -}}
{{- end -}}
{{- if le $bytes 0.0 -}}
{{ fail (printf "resources.limits.memory %q is not a quantity this chart can derive runtime.workingSetMaxBytes from - set runtime.workingSetMaxBytes explicitly" $mem) }}
{{- end -}}
{{- floor (mulf $bytes 0.75) | int64 -}}
{{- end -}}
{{- end -}}
