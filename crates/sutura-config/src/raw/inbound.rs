/// The inbound-identity declaration, as read.
///
/// **Flat, with every key optional, and the mode is what decides which are required.** The
/// alternative - a tagged enum in serde - reads better and diagnoses worse: `config` layers a variable
/// per key, so `SUTURA__SECURITY__INBOUND__RESOURCE` has to be settable without the layer below it
/// having to restate the mode. Every mode-dependent absence becomes a refusal naming the key, in
/// `crate::settings::parse_inbound`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "three are an operator's acceptance by name and one a switch, each read once at load"
)]
pub(crate) struct RawInbound {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) verifier: Option<String>,
    #[serde(default)]
    pub(crate) client_id: Option<String>,
    #[serde(default)]
    pub(crate) hosted_domains: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) required_scopes: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) tokeninfo_endpoint: Option<String>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    /// Both modes: the token size bound, excluding the scheme.
    #[serde(default)]
    pub(crate) max_token_bytes: Option<usize>,
    /// `direct` or `behind-gateway`. **Required**, and there is no default because both would be
    /// wrong - see `crate::inbound`.
    #[serde(default)]
    pub(crate) mode: Option<String>,
    /// `direct`: what this deployment calls itself when it validates an audience.
    #[serde(default)]
    pub(crate) resource: Option<String>,
    /// `direct`: where the tokens it accepts are minted.
    #[serde(default)]
    pub(crate) authorization_server: Option<String>,
    /// `behind-gateway`: the header the signed transit proof arrives in. Never a header holding a
    /// name - see `crate::inbound` on why this is a proof and not an assertion.
    #[serde(default)]
    pub(crate) transit_header: Option<String>,
    /// `behind-gateway`: the issuer that must have signed the proof.
    #[serde(default)]
    pub(crate) transit_issuer: Option<String>,
    /// `behind-gateway`: the audience the proof must carry - an `https` URI or an absolute path.
    #[serde(default)]
    pub(crate) transit_audience: Option<String>,
    /// `direct`: what `aud` must equal, where it is not `resource` - an Entra application id.
    #[serde(default)]
    pub(crate) audience: Option<String>,
    /// `direct`: which claims beside `scope` grant scopes, out of `scp` and `roles`.
    #[serde(default)]
    pub(crate) extra_scope_claims: Option<Vec<String>>,
    /// `behind-gateway`: the scopes an assertion with no `scope` claim is granted.
    #[serde(default)]
    pub(crate) granted_scopes: Option<Vec<String>>,
    /// Both modes: where the signing keys are read from. Exactly one of this and `key_set_url`.
    #[serde(default)]
    pub(crate) key_set_file: Option<String>,
    /// Both modes: the `https` URL the signing keys are fetched from.
    #[serde(default)]
    pub(crate) key_set_url: Option<String>,
    /// Both modes: how long the keys verify after a look last confirmed them, in seconds. Absent
    /// means `KeySetMaxStale::DEFAULT_SECONDS`.
    #[serde(default)]
    pub(crate) key_set_max_stale_seconds: Option<u64>,
    /// Both modes: the algorithms this deployment will accept. Required and never defaulted, because
    /// pinning is the control and a default here would be this crate choosing it.
    #[serde(default)]
    pub(crate) algorithms: Vec<String>,
    /// `direct`: which class of token, out of the `typ` header. Absent means RFC 9068's `at+jwt`.
    ///
    /// **Absent is the SAFE value here, unlike `mode`**, which is why it has a default at all: the
    /// unsafe reading is `any`, and that is a word an operator writes, accepts by name with
    /// `accept_any_token_type`, and the startup log prints at `WARN`. Defaulting the other way would
    /// have made the check switchable by silence, which is the shape review found.
    #[serde(default)]
    pub(crate) token_type: Option<String>,
    /// `direct`: the operator's by-name acceptance of `token_type: any`, without which it does not
    /// start.
    #[serde(default)]
    pub(crate) accept_any_token_type: bool,
    /// `behind-gateway`: which class of token the component emits. **Required**, because a component's
    /// `typ` is a fact only the deployment knows - there is no value this crate could guess that does
    /// not either reject every request or check nothing. `any` is how a deployment says its component
    /// sets none.
    #[serde(default)]
    pub(crate) transit_token_type: Option<String>,
    /// `behind-gateway`: the longest lifetime a proof may declare, in seconds. Absent means
    /// `ProofLifetime::DEFAULT_SECONDS`.
    #[serde(default)]
    pub(crate) transit_max_lifetime_seconds: Option<u64>,
    /// `behind-gateway`: the operator's by-name acceptance of a lifetime above
    /// `ProofLifetime::SHORT_LIVED_SECONDS`, without which it does not start.
    #[serde(default)]
    pub(crate) accept_long_transit_lifetime: bool,
    /// `behind-gateway`: admit each assertion once, by its `jti`. Off by default - `docs/adr/20261015094920`.
    #[serde(default)]
    pub(crate) transit_single_use: bool,
    /// Both modes: the claim a `domain_delegation` source reads the caller's address from. Absent
    /// means `email` beside `email_verified: true`.
    #[serde(default)]
    pub(crate) address_claim: Option<String>,
    /// Both modes: the operator's by-name acceptance of a claim other than `email`, without which it
    /// does not start.
    #[serde(default)]
    pub(crate) accept_address_claim: bool,
}
