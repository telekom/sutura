---
title: The agent surface without an inbound identity on a single-user deployment
description: When `/mcp` may run with no `security.inbound` - a declared single-user deployment, reachable from loopback only or behind the deployment token and the limiter, with no source that runs as the asking subject - and the two startup refusals that hold every other deployment to leg 1 (telekom/sutura#1293).
---

# The agent surface without an inbound identity on a single-user deployment

Status: **accepted** (issue #1293). It narrows one row of `docs/adr/0023`'s acceptance bar, "no
verified caller on the request: refuse", for the one deployment named below. Every other deployment
keeps that row.

## The question

`/mcp` over HTTP was a startup refusal on every deployment with no `security.inbound`, while `/v1`
on the same deployment answers every caller as the deployment. One operator on their own machine
had to run an identity provider to reach the agent surface over HTTP, or use the stdio surface,
which has none of the HTTP controls. Where may `/mcp` answer as the deployment, and what holds every
other deployment to leg 1?

## Decision

`/mcp` runs with no `security.inbound` only when all three hold:

1. `security.identity: single-user` is declared, which `DeploymentIdentity` accepts only with its
   `security.single_user_because` reason. **A missing mode is not single-user**, so a deployment
   that declares nothing keeps the refusal.
2. The bind is loopback, or both `security.access_token` and `rate_limit.enabled` guard it. The
   first amendment widens what counts as off-host.
3. No configured source declares `impersonation-at-source`.

Two startup refusals in `NotFitToServe` (`crates/sutura-config/src/settings/posture.rs`), returned
by `Settings::agent_surface_refusals` (`crates/sutura-config/src/settings.rs`) and reported by
`Settings::refusals` where `server.agent_surface.enabled` is set:

| Refusal                                           | Fires when                                                |
| ------------------------------------------------- | --------------------------------------------------------- |
| `AgentSurfaceWithoutInboundIdentity`              | no `security.inbound`, and not both (1) and (2)           |
| `AgentSurfaceOverAnImpersonatingSource { alias }` | no `security.inbound`, once for each source (3) rules out |

Where `security.inbound` is declared neither fires, and `/mcp` runs behind leg 1 as before.

**(3) is read off the declared posture, not off a list of kinds.** The check asks each source's
`SourcePosture::deliverable_by(ImpersonationCapability::NoPlaceForASubject, ..)`: a posture that
only an adapter with a place for a subject can deliver needs a verified subject. That match is
exhaustive with no wildcard arm, so a new source kind is caught by the posture it declares, and a
new posture does not compile until it answers. No trait item and no type was added for it.

**(2) is stated inside the refusal**, though `AccessTokenRequired` and `RateLimitingDisabled`
already refuse the same off-host shapes, so this rule does not depend on those two staying as they
are.

## The assembly door

`Settings::refusals` is keyed on `server.agent_surface.enabled`. `sutura_http`'s assembly mounts
whatever `ServiceState::with_agent_surface` attached, and no type ties that mount to the switch.

- **Delete the assembly check** and rely on `Settings::load`. Smaller, but a composition root that
  attaches a mount without setting the switch would then serve `/mcp` under no rule at all.
- **Keep it, over one predicate. Chosen.** `agent_subtree` (`crates/sutura-http/src/router.rs`)
  asks the same `Settings::agent_surface_refusals` for any attached mount and refuses with
  `RouterNotBuilt::AgentSurfaceNotFitToServe { refusals }`, which replaces
  `RouterNotBuilt::AgentSurfaceWithoutInboundIdentity`. One rule, two call sites, so the two cannot
  disagree about which deployment may serve `/mcp` without leg 1.

## Proof

| Cell                                                                                                                         | Shows                                                                                             |
| ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `settings::tests::agent_surface::an_agent_surface_with_no_inbound_identity_in_multi_user_mode_is_not_fit_to_serve`           | `multi-user` is refused; the same deployment behind leg 1 serves                                  |
| `settings::tests::agent_surface::an_agent_surface_with_no_inbound_identity_and_no_declared_mode_is_not_fit_to_serve`         | no mode is refused; `single-user` without its reason is not a mode; `single-user` loopback serves |
| `settings::tests::agent_surface::an_off_host_agent_surface_with_no_inbound_identity_needs_the_token_and_the_limiter`         | off-host, each of the two guards is required                                                      |
| `settings::tests::agent_surface::an_agent_surface_over_an_impersonating_source_with_no_inbound_identity_is_not_fit_to_serve` | refused per source, and only for the source that runs as the asker                                |
| `router::tests::a_mounted_agent_surface_is_refused_where_the_deployment_may_not_serve_it_without_leg_one`                    | the assembly door refuses a mount whose switch was never set                                      |
| `served::agent::a_multi_user_agent_surface_with_no_inbound_identity_stops_the_process`                                       | the composed binary exits before it binds                                                         |
| `served::agent::a_single_user_loopback_agent_surface_with_no_inbound_identity_answers_as_the_deployment`                     | the composed binary serves every tool the deployment turns on, with no token                      |

## Limits

- **`single-user` is declared, not measured.** It is a word and a reason the operator writes;
  nothing counts who calls. `RunSqlEnabledInMultiUserMode` carries the same limit.
- Such a `/mcp` answers every caller as the deployment, with every capability the deployment turns on
  (`run_sql` only while `tools.run_sql.enabled` is on). No scope narrows the
  tool list, because there is no verified caller to read a scope from. That is `/v1`'s posture on
  the same deployment.
- **`/v1` is unchanged by this record.** (3) is scoped to `/mcp`, the surface this record opens; the
  third amendment below holds `/v1` to leg 1 too.
- The refusals read the configuration, not the command, so `sutura mcp` over stdio reading a file
  that sets `server.agent_surface.enabled` is refused by them too, as by every other `NotFitToServe`.
- (2) reads the configuration, not the traffic. The first amendment below sets what it counts as
  off-host.
- (2) accepts any enabled limiter; it does not judge its tier.

## First amendment, 2026-10-08: the agent surface counts a declared proxy, TLS terminator or non-loopback host name as off-host

**Status of the amendment: accepted** (issue #1293). (2) first read the bind address alone. For the
agent surface it now counts four things as off-host, and each one alone does:

- a bind that is not loopback;
- a `rate_limit.trusted_proxies` hop, which says callers arrive through a proxy;
- a `security.tls_termination` of `sidecar` or `ingress`, which names a terminator that accepts
  callers for this process; `none` names none, and `in-process` terminates here;
- a `server.allowed_hosts` entry that `sutura_domain::source::host_is_loopback` does not accept,
  which says callers reach this deployment by a name. That is the one shared loopback predicate,
  and a name is not an address, so `localhost` counts as off-host too.

Off-host, (2) still asks for both `security.access_token` and `rate_limit.enabled`. The refusal
stays `AgentSurfaceWithoutInboundIdentity`, from the same `Settings::agent_surface_refusals`, so
`Settings::refusals` and `agent_subtree` cannot disagree; no variant was added.

| Cell                                                                                                           | Shows                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `settings::tests::agent_surface::a_loopback_agent_surface_behind_a_declared_proxy_counts_as_off_host`          | a loopback bind with a proxy hop is refused; with the token and the limiter it serves                                                  |
| `settings::tests::agent_surface::a_loopback_agent_surface_answering_a_host_name_counts_as_off_host`            | a name, `localhost`, a non-loopback address and a name beside a loopback address are refused; loopback addresses and both guards serve |
| `settings::tests::agent_surface::a_loopback_agent_surface_behind_a_declared_tls_terminator_counts_as_off_host` | `sidecar` and `ingress` are refused; with the token and the limiter each serves, and `none` serves                                     |
| `router::tests::a_mounted_agent_surface_is_refused_where_the_deployment_may_not_serve_it_without_leg_one`      | the assembly door refuses the proxy and the host-name shapes on a mount whose switch was never set                                     |
| `served::agent::a_single_user_loopback_agent_surface_behind_a_declared_tls_terminator_stops_the_process`       | the composed binary exits before it binds, for `sidecar` and for `ingress`                                                             |

Limits:

- **`/v1` is unchanged.** Its `AccessTokenRequired` and `RateLimitingDisabled` still read the bind
  alone.
- The rule reads what the operator declared. A hop the configuration does not name is outside it.

## Second amendment, 2026-10-11: `/v1` and `/metrics` ask the same off-host predicate

**Status of the amendment: accepted** (issue #1293). The first amendment widened off-host for the
agent surface only, so a loopback bind behind a declared proxy or terminator served `/v1` with no
token and no limiter while `/mcp` was refused. The four conditions of the first amendment are now
one method, `Settings::reachable_off_host`, and every refusal that guards a surface against callers
from beyond this host asks it:

| Refusal                              | Predicate | Why                                                                                          |
| ------------------------------------ | --------- | -------------------------------------------------------------------------------------------- |
| `AccessTokenRequired`                | off-host  | a declared hop forwards callers from other hosts to `/v1`                                    |
| `MetricsTokenRequired`               | off-host  | `/metrics` is on the same listener, so the same callers reach it                             |
| `RateLimitingDisabled`               | off-host  | the load is somebody else's to send whatever the bind is                                     |
| `AgentSurfaceWithoutInboundIdentity` | off-host  | unchanged; now the same method                                                               |
| `TlsTerminationUndeclared`           | the bind  | it records the cleartext hop in front of this listener, which is loopback on a loopback bind |

`TokenRequiredBy::OffHost` names the four conditions in its message. Because an off-host shape with
no token or no limiter no longer loads, the assembly door of this record never meets one: its cell
refuses at assembly a deployment with no mode and a `multi-user` one, and the
proxy and host-name cells of `settings::tests::agent_surface` hold that those shapes do not load. The
`MetricsTokenRequired` row of `docs/adr/0015` still reads "on a non-loopback bind"; it now means
off-host.

| Cell                                                                                                          | Shows                                                                                        |
| ------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `settings::tests::off_host::a_loopback_api_behind_a_declared_proxy_needs_the_tokens_and_the_limiter`          | a proxy hop asks for both tokens and the limiter; with all three it loads                    |
| `settings::tests::off_host::a_loopback_api_behind_a_declared_tls_terminator_needs_the_tokens_and_the_limiter` | `sidecar` and `ingress` ask for all three; `none` loads with none                            |
| `settings::tests::off_host::a_loopback_api_answering_a_host_name_needs_the_tokens_and_the_limiter`            | a name, `localhost` and a name beside a loopback address ask for all three; addresses do not |
| `settings::tests::agent_surface::a_loopback_agent_surface_behind_a_declared_proxy_counts_as_off_host`         | the agent refusal and the three others arrive together                                       |

Limits:

- The rule still reads what the operator declared. A hop the configuration does not name is outside
  it.
- A loopback bind behind a declared proxy with no `security.tls_termination` starts: what sits in
  front of that proxy is the proxy's own configuration.

## Third amendment, 2026-10-15: `/v1` is refused without an inbound identity at the serve root

**Status of the amendment: accepted** (issue #1293). With no `security.inbound`, `/v1` answers every
caller as the deployment, so `sutura serve` now refuses to start it unless `security.identity` is
`single-user` with its reason, and refuses it over any source whose posture is
`impersonation-at-source`, which has no verified caller to run as:

| Refusal                                 | Refuses                                                                       |
| --------------------------------------- | ----------------------------------------------------------------------------- |
| `QuerySurfaceWithoutInboundIdentity`    | no `security.inbound` and a `security.identity` that is not `single-user`     |
| `QuerySurfaceOverAnImpersonatingSource` | no `security.inbound` and a source with `impersonation-at-source`, per source |

Both come from `Settings::query_surface_refusals`, asked where `sutura serve` starts `/v1` and not
by `Settings::refusals`: no key mounts `/v1`, so every command that loads settings would be refused
by it. A deployment with no `security.inbound` and an impersonating source therefore no longer
starts, and `/v1` no longer answers `credential_unavailable` for it.

| Cell                                                                                    | Shows                                                      |
| --------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `serve::tests::v1_with_no_inbound_identity_starts_only_on_a_single_user_deployment`     | no mode and `multi-user` are refused; `single-user` starts |
| `serve::tests::v1_with_no_inbound_identity_does_not_start_over_an_impersonating_source` | the impersonating source is refused; a shared one starts   |

Limits:

- The rule is asked at the serve root only. `sutura_http::router` does not ask it, so a router built
  in process with no inbound gate is not refused; the `/mcp` rule is asked by the router's
  `agent_subtree`.
- `single-user` is declared, not measured, as above.

## Fourth amendment, 2026-10-16: the router takes settings that passed the `/v1` check

**Status of the amendment: accepted** (PR #1465). The third amendment's first limit is closed for an
embedder: `sutura_http::ServiceState::new`, which `sutura_http::router` is assembled from, takes a
`ServableSettings` and not bare settings, and only `ServableSettings::check` builds one. `check`
asks `Settings::query_surface_refusals` of a deployment with no `security.inbound` and returns every
reason it gives, as `NotServable`. A deployment that declares `security.inbound` passes without the
rule, because the rule is about the deployment that does not; that deployment's gate is held at
assembly by `RouterNotBuilt::InboundIdentityNotAttached`. `sutura serve` still refuses first, in
`inbound_gate`, and `check` there refuses nothing new.

| Cell                                                                                                                   | Shows                                                                  |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `servable::tests::a_deployment_with_no_inbound_identity_and_no_impersonating_source_is_servable_only_when_single_user` | no mode and `multi-user` are refused; `single-user` is servable        |
| `servable::tests::a_deployment_with_no_inbound_identity_over_an_impersonating_source_is_not_servable`                  | single-user is refused for the source; multi-user carries both reasons |
| `servable::tests::a_deployment_that_declares_inbound_identity_is_servable_without_the_rule`                            | a declared inbound identity passes                                     |
| `ServableSettings`' `compile_fail,E0423` doctest                                                                       | the private field cannot be built from outside the crate               |

Limits:

- **The guard is the type only in a build without `sutura-http/fixtures`.** With it on, a bare
  `Arc<Settings>` converts to `ServableSettings`. A dev edge from `sutura-cli` turns it on, and the
  Rust legs of `just lint`, `just test` and `just validate` all pass `--all-features`. The
  `--all-targets` passes of `just check-default-features` do not; they unify that dev edge. A
  revert of the `check` call at the serve root therefore compiles in all of them. Only a build
  with no dev edges refuses it: the
  `cross` link legs, `just shipped` and the release build. That rests on `nix/shipped.nix` building
  `--package` with no dev edges and on a `cargo check -p sutura-cli` of the reverted call, which
  gave `E0277`; the nix cross builds were not run.
- `check-default-features` refuses `sutura-http/fixtures` in a shipped root's normal dependencies. It
  resolves the default feature set, not the `--features` list a release passes, so a feature turned
  on only there is not seen.
- No `compile_fail` cell holds that the `From<Arc<Settings>>` impl is absent without the feature:
  doctests run with `--all-features`. The `E0423` doctest holds only the private field.
- `check` takes `Settings` by value, so a caller that holds an `Arc<Settings>` has to clone it.
- The `/mcp` rule is not part of this type: it is asked by the router's `agent_subtree`.
- `single-user` is declared, not measured, as above.
