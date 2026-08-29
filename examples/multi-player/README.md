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

**The port now exists**, and it arrived the way this repository requires - with an adapter
that implements it rather than as a guess at a signature.
`sutura_domain::identity::CredentialBroker` mints once per answer for every source a plan
reads, `Warehouse::execute` cannot be called without the result, and
`sutura_config::StaticCredentialBroker` is the implementor: credentials as configuration,
one user, one host, which is the single-player deployment mode rather than test scaffolding.

**What is missing is the other half, and it is the half this example is about: a data system
with identities to run under.** Both adapters in this build declare that they have nowhere
for a subject's own credential to arrive - one process reading local files, one process
holding one connection - so what a broker can mint here is the deployment's own identity for
a source, acknowledged by an operator. A source declared `impersonation-at-source` gets no
credential from the shipped broker and its questions are refused as `credential_unavailable`,
which is the *refusal instead of a downgrade* above, arriving before the impersonation does.

So the two bullets this example turns on are still unrunnable: **two callers, two answers**
needs a data system that evaluates two principals differently, and nothing here can present
one to it. `docs/architecture.md` is the design and
`docs/adr/0008-a-credential-per-leg-for-the-calling-subject.md`'s *What is built* is the
inventory. No date is offered here, because a date in a README is not a commitment anything
enforces.
