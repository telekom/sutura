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
2. The bind is loopback, or both `security.access_token` and `rate_limit.enabled` guard it.
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
| `served::agent::a_single_user_loopback_agent_surface_with_no_inbound_identity_answers_as_the_deployment`                     | the composed binary serves every tool, with no token                                              |

## Limits

- **`single-user` is declared, not measured.** It is a word and a reason the operator writes;
  nothing counts who calls. `RunSqlEnabledInMultiUserMode` carries the same limit.
- Such a `/mcp` answers every caller as the deployment, with every capability. No scope narrows the
  tool list, because there is no verified caller to read a scope from. That is `/v1`'s posture on
  the same deployment.
- **`/v1` is unchanged.** A deployment with no `security.inbound` and an impersonating source still
  starts, and `/v1` answers each question `credential_unavailable`. (3) is scoped to `/mcp`, the
  surface this record opens.
- The refusals read the configuration, not the command, so `sutura mcp` over stdio reading a file
  that sets `server.agent_surface.enabled` is refused by them too, as by every other `NotFitToServe`.
- (2) accepts any enabled limiter; it does not judge its tier.
