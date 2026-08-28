---
title: A raw SQL tool, off by default, and the ramp it exists to build
description: Why a deployment may enable a general SQL tool beside the certified one - because a database with DDL and comments and no semantic layer is where every adoption starts, and refusing everything until somebody commits to governance is how a governed product never gets adopted - and the mechanisms that keep it from ever looking certified: a separate scope, a result type that cannot carry a definition digest, read-only enforced by the source rather than by parsing, and coverage reported as a number so the ungoverned path cannot quietly become the default.
---

# A raw SQL tool, off by default, and the ramp it exists to build

Status: **accepted, and it amends an invariant.** Nothing here is built.

This record reverses part of a rule that was previously absolute: *no SQL on the tool surface.* The rule
was right about the certified path and wrong as a statement about the whole product, and the reason is
adoption rather than convenience.

## Why, and it is not "someone asked for it"

**A database with DDL and comments and no semantic layer is where every adoption starts.** Not an edge
case, the normal case. The semantic layer is the valuable thing and it is also organisational work -
somebody has to own a number and say what it means - and nobody funds that work before seeing the tool
answer a question.

So a product that refuses everything until a semantic layer exists asks for the commitment first and
delivers the value second. That order does not survive contact with a sceptical sponsor, and the failure
is not that they disagree about governance: it is that they never see it work.

**Three things this has to demonstrate, in order:**

1. **It works.** Real questions, real data, from what a database already documents about itself.
2. **Defining a metric is smooth.** The step from "that answer was useful" to "that answer is certified"
   has to be short, and the system should carry the candidate rather than leaving somebody to write it
   from scratch.
3. **Coverage grows, visibly.** More certified metrics means more questions answered the governed way,
   and that shift is a number somebody can see rather than a claim.

The third one is what stops this record from being a hole in the design. An ungoverned path with no
measurement becomes the comfortable default. An ungoverned path whose share is reported is a ramp.

## The mechanisms

**Off by default, per deployment.** Enabling it is a configuration decision that shows in a diff, and a
deployment that never enables it behaves exactly as before this record.

**A separate tool with its own scope, never a field on the certified one.** The typed question keeps
having no field for SQL, a table, a predicate or row ids. This is a second tool, and a caller without the
scope **does not see it advertised** - invisible rather than rejected, because a tool nobody may call
should not be in the list.

**Its result cannot claim certification, and that is a type rather than a rule.** The certified answer
carries provenance with no constructor that omits it. The raw tool returns a **different outcome type,
which has no field for a definition digest at all**, so labelling a raw result as certified is
unrepresentable rather than forbidden. That is the load-bearing mechanism in this record.

**Read-only is enforced by the SOURCE, not by us.** The tool may only be enabled over a credential or
role that cannot write. Deciding read-only-ness by inspecting the statement would mean parsing it, and a
parser that has to be right about every dialect's write forms is exactly the fragile thing this codebase
refuses elsewhere. Let the database say no.

**Impersonation is required unless the deployment is single-user.** This is the sharpest constraint and
it follows from what the tool is: arbitrary SQL under a shared service account reads everything that
account can see, for anybody who can call the tool. So the raw tool is available where the source
executes as the asking subject - and in single-user deployments, which are development and
proof-of-concept shapes with one user and static credentials. Nowhere else.

**Every bound applies unchanged:** the result ceiling, the query timeout, the row cap. A raw statement is
not a reason to be unbounded; if anything it is the path most likely to need them.

**One property genuinely does not hold, and pretending otherwise would be the defect.** "No caller text
reaches a statement" is false for this tool by construction - the statement *is* caller text. Its safety
therefore comes from the source's own authorization, which is why the previous rule is not optional.

On the certified path the guarantee is untouched and keeps its own mechanism: every value from a
question becomes a **bind parameter**, statement and parameters are separate fields with no merging
constructor, and a golden asserts no question literal appears in a statement. Behind that, a filter
value must also be a member of the catalog's declared allowlist - defence in depth, not the guarantee
itself. Neither is weakened by this record, because the raw tool is a different tool with a different
result type rather than a wider version of this one.

## The ramp, as mechanism rather than intention

**Every ungoverned answer is a recorded demand signal.** The question that was answered without a
certified metric is the most valuable input the backlog has: it is somebody's real question, in their
words, with the SQL that answered it. Refusals already carry this role on the certified path; this
extends it.

**The candidate definition travels with the answer.** Where the shape allows it - a single aggregate over
one table at one grain - the tool can emit a draft metric definition alongside the rows: the model, the
measure, the grain. A human reviews and commits it. That is the difference between "define your metrics"
as advice and as a workflow.

**Coverage is reported.** The share of questions answered certified against ungoverned, over time, per
deployment. Rising is the whole point, and a deployment where it does not rise has learned something
worth knowing.

**And the labelling is not decoration.** An ungoverned answer says so, in the payload, every time. A
reader who cannot tell which kind of answer they are holding has the worst of both designs.

## What this changes in the invariants

The row reading *"No SQL, table name, filter expression or row-id list on the tool surface"* becomes a
statement about the **certified** surface, and gains the second half: a raw tool exists only as a
separately-scoped, off-by-default capability whose result type cannot carry a definition digest, and
which is available only where the source executes as the asking subject or the deployment is
single-user. The mechanism is unchanged for the typed question - `deny_unknown_fields` still makes an
extra field an error naming it - and the new mechanism for the raw tool is the outcome type that has
nowhere to put a digest.

**What is NOT weakened:** a certified answer is still produced by compiling a declared metric, not by a
model writing SQL. That is the point of the whole system and this record does not touch it. A deployment
with both paths has two clearly distinguished paths; it does not have a blurrier version of the certified
one.

## Consequences

- Two outcome types on the surface, and the difference between them is visible to a caller rather than
  documented. That is the intended cost.
- The raw tool cannot be tested by the conformance packs that assert certified behaviour, because it
  asserts different things. It needs its own small pack: that it is absent without the scope, that its
  result cannot carry a digest, that a write is refused by the source, and that the bounds bite.
- A deployment enabling it over a shared service account in multi-user mode must be refused at startup,
  for the same reason and by the same mechanism as a source declaring an impersonation it cannot perform.
- The demo that motivated this becomes reproducible: catalog search plus a select, with the answers
  labelled for what they are, and a path from there to a metric that makes the next identical question
  certified.
