/* TIER validator: authorises exactly the bearer "sutura-tier-bearer-<role>" for <role>. It checks a
 * fixed string, never a JWT. A real deployment's validator verifies a token against its issuer; this
 * one only closes the protocol loop, and the operator owns the deployment validator. A deployment
 * must write its own, on the server side; this module exists only to let the tier's test sign in. */
#include "postgres.h"
#include "fmgr.h"
#include "libpq/oauth.h"
PG_MODULE_MAGIC;
static bool validate(const ValidatorModuleState *s, const char *token, const char *role, ValidatorModuleResult *r) {
  char want[256];
  snprintf(want, sizeof want, "sutura-tier-bearer-%s", role);
  r->authorized = strcmp(token, want) == 0;
  r->authn_id = r->authorized ? pstrdup(role) : NULL;
  return true;
}
static const OAuthValidatorCallbacks cb = { PG_OAUTH_VALIDATOR_MAGIC, NULL, NULL, validate };
const OAuthValidatorCallbacks *_PG_oauth_validator_module_init(void) { return &cb; }
