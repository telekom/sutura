{{/*
Naming, common labels, and the render-time guards that mirror `Settings::refusals` -
`crates/sutura-config/src/settings.rs` - close enough to catch a broken values file before
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
always asks for an access token, a declared TLS termination, and an enabled rate limiter -
this fails the render rather than shipping a Deployment that crash-loops on the first refusal.
*/}}
{{- define "sutura.requireOffHostPosture" -}}
{{- if not .Values.environment -}}
{{ fail "environment is required (SUTURA_ENVIRONMENT) - see values.yaml" }}
{{- end -}}
{{- if not .Values.security.accessToken.secretName -}}
{{ fail "security.accessToken.secretName is required: this chart's Service makes the pod reachable off-host, and sutura refuses to start with no security.access_token there (Settings::refusals::AccessTokenRequired)" }}
{{- end -}}
{{- $tlsDeclared := or .Values.tls.enabled (ne .Values.security.tlsTermination "") -}}
{{- if not $tlsDeclared -}}
{{ fail "set tls.enabled=true or security.tlsTermination (sidecar|ingress|in-process): sutura refuses to start off-host with security.tls_termination undeclared (Settings::refusals::TlsTerminationUndeclared)" }}
{{- end -}}
{{- if and .Values.tls.enabled (not .Values.tls.secretName) -}}
{{ fail "tls.enabled requires tls.secretName (a kubernetes.io/tls Secret)" }}
{{- end -}}
{{- end -}}
