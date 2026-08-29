# Multi player

Empty on purpose. The example that belongs here cannot be written yet, and this file says
what it would be and what is missing.

## Why single player proves less than it looks like

`examples/single-player` is a catalog in git, a CSV per model, and whatever access the
process already had. Every claim in it holds. The one claim that matters most holds for a
reason that does not generalise: "every query runs as the calling principal" is trivially
true there, because a local file has no login to present and there is nobody else to be.

That is a true statement about a laptop and not about a warehouse. On a data system with
grants, the same question asked by two people is two different sets of rows, and a runtime
that reads them under its own service identity has turned a row-level security policy into
decoration. Nothing in the single-player example can tell the two apart, so nothing in it
tests the difference.

## What will be here

An example is only worth writing once it can fail. This one needs three things it can
demonstrate:

- **Per-request credentials.** A credential minted for the caller of this request, for this
  leg of this plan, rather than a connection opened at startup and shared.
- **Two callers, two answers.** The same certified question, the same catalog, the same
  data system, and two different result sets, because the two principals may read different
  rows. That is the assertion the example exists for, and it needs at least two identities
  to be worth running.
- **A refusal instead of a downgrade.** A leg that cannot run as the subject comes back as
  a refusal naming that, never as an answer computed under some other identity. The failure
  mode being guarded against is the silent one: an answer that looks correct and was read by
  the wrong principal.

## What is missing

The port. `CredentialBroker` does not exist, and its absence is deliberate rather than
pending: in this repository a port trait arrives with the adapter that implements it,
because a trait with no implementor is a guess at a signature. Nothing mints per-request
credentials yet, so there is nothing for the trait to be shaped by, and no example here
could do more than describe an intention.

**What is no longer missing is the plumbing**, and it is worth saying so here because it is the
part somebody writing this example would otherwise build again. The two databases this example
needs are one command away - `just dev-up` - one independent instance per worktree, and a harness
reaches them through `sutura_dev::provisioned` rather than through a port anybody wrote down. So
the work left is the port and the two identities, not the fixtures: `examples/README.md` has the
three commands under *Reaching a data system, when an example needs one*.

It arrives with the first data system that has identities to run under. `docs/architecture.md`
is the design: the security section says why the shape is what it is, and "What exists
today" is the honest inventory of which parts are built. No date is offered here, because a
date in a README is not a commitment anything enforces.
