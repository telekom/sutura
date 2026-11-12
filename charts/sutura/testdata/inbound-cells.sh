#!/usr/bin/env bash
# Render the chart's own helpers: the expected declaration must reach every reading worker.
set -euo pipefail
chart="$1"
work="$2/inbound-cells"
mkdir -p "$work"
cp -r --no-preserve=mode "$chart" "$work/chart"
chart="$work/chart"
cat > "$chart/templates/inbound-observation.yaml" <<'TEMPLATE'
{{- if .Values.inboundObservation -}}
{{- range $name, $group := .Values.readingWorkers.groups -}}
{{- $worker := include "sutura.readingWorkerSettings" (dict "root" $ "group" $group) | fromYaml -}}
{{- if not (deepEqual (get $worker.security "inbound") $.Values.inboundExpected) -}}
{{- fail (printf "inbound/%s: reader %s lost or changed its declaration" $.Values.inboundCase $name) -}}
{{- end -}}
{{- if ne (len $worker.sources) (len $group.sources) -}}
{{- fail (printf "inbound/%s: reader %s has another group's sources" $.Values.inboundCase $name) -}}
{{- end -}}
{{- range $alias := $group.sources -}}
{{- if not (deepEqual (get $worker.sources $alias).admitted_by $.Values.inboundExpectedAdmissions) -}}
{{- fail (printf "inbound/%s: reader %s lost source %s admission ids" $.Values.inboundCase $name $alias) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- end -}}
TEMPLATE

primary='{"id":"staff","path":"/staff/mcp","mode":"direct","resource":"https://sutura.example.com/staff/mcp","authorization_server":"https://issuer.example.com","key_set_file":"/etc/sutura/staff.json","algorithms":["RS256"]}'
gateway='{"id":"agents","path":"/agents/mcp","mode":"behind-gateway","transit_header":"X-Transit-Proof","transit_issuer":"https://gateway.example.com","transit_audience":"https://sutura.example.com/agents/mcp","transit_token_type":"at+jwt","key_set_file":"/etc/sutura/agents.json","algorithms":["ES256"]}'
legacy='{"mode":"direct","resource":"https://sutura.example.com","authorization_server":"https://issuer.example.com","key_set_file":"/etc/sutura/legacy.json","algorithms":["RS256"]}'

values() {
  local ids="${4:-[\"primary\"]}"
  printf '{"inboundCase":"%s","inboundExpectedAdmissions":%s,"config":{"base":{"security":{"inbound":%s},"sources":{"orders":{"admitted_by":%s},"billing":{"admitted_by":%s},"ledger":{"admitted_by":%s},"inventory":{"admitted_by":%s}}},"environment":{"security":%s}}}\n' \
    "$1" "$ids" "$2" "$ids" "$ids" "$ids" "$ids" "$3" > "$work/values.json"
}
render() {
  helm template test-release "$chart" -f "$chart/testdata/values/pool/reading-workers.yaml" \
    --set security.inbound.keySetSecretName= -f "$work/values.json" "$@"
}
accepts() {
  local name="$1" base="$2" env="$3" expected="$4" ids="$5"
  values "$name" "$base" "$env" "$ids"
  if ! render --set inboundObservation=true --set-json "inboundExpected=$expected" > "$work/$name.yaml" 2> "$work/$name.log"; then
    cat "$work/$name.log" >&2
    echo "inbound/$name: expected a render preserving the complete declaration" >&2
    exit 1
  fi
  echo "inbound/$name: PASS"
}
refuses() {
  local name="$1" base="$2" expected="$3"
  shift 3
  values "$name" "$base" '{}'
  if render "$@" > "$work/$name.yaml" 2> "$work/$name.log"; then
    echo "inbound/$name: expected refusal $expected" >&2
    exit 1
  fi
  if ! grep -F "$expected" "$work/$name.log"; then
    cat "$work/$name.log" >&2
    echo "inbound/$name: wrong refusal, expected $expected" >&2
    exit 1
  fi
  echo "inbound/$name: PASS"
}

accepts base-list "[$primary,$gateway]" '{}' "[$primary,$gateway]" '["staff","agents"]'
accepts list-over-map "$legacy" "{\"inbound\":[$primary,$gateway]}" "[$primary,$gateway]" '["staff","agents"]'
accepts map-over-list "[$primary,$gateway]" "{\"inbound\":$legacy}" "$legacy" '["primary"]'
accepts list-over-list "[$primary]" "{\"inbound\":[$gateway]}" "[$gateway]" '["agents"]'
accepts map-merge "$legacy" '{"inbound":{"resource":"https://other.example.com"}}' \
  '{"mode":"direct","resource":"https://other.example.com","authorization_server":"https://issuer.example.com","key_set_file":"/etc/sutura/legacy.json","algorithms":["RS256"]}' '["primary"]'
refuses later-direct "[$gateway,$primary]" DeploymentTokenSharesTheHeader --set security.accessToken.secretName=caller-token
refuses later-gateway-header "[$gateway,{\"mode\":\"behind-gateway\",\"transit_header\":\"authorization\"}]" \
  DeploymentTokenSharesTheHeader --set security.accessToken.secretName=caller-token
refuses later-missing-mode "[$gateway,{}]" InboundModeUndeclared
refuses later-unknown-mode "[$gateway,{\"mode\":\"unknown\"}]" InboundModeUnknown
refuses later-invalid-header "[$gateway,{\"mode\":\"behind-gateway\",\"transit_header\":\"bad header\"}]" NotPermitted
refuses empty-list '[]' InboundListEmpty
refuses list-with-legacy-secret "[$primary,$gateway]" 'sets one legacy inbound object' --set security.inbound.keySetSecretName=legacy-keys
