---
name: autoreason
description: Make the argument explicit before acting on it — state the claim, what would falsify it, and check it against the repo rather than against memory.
---

# Autoreason

For decisions that are expensive to reverse, and for conclusions that arrived too easily.
Skip it for mechanical work.

## The loop

**1. State the claim as one sentence.** If you cannot, you do not have one yet — you have a
direction. Say that instead.

**2. Say what would falsify it.** A claim with no falsifier is a preference. Write the
specific observation that would change your mind: a test that would fail, a file that would
not exist, an error the system would emit.

**3. Check the falsifier against the repo, not your memory.** Read the file. Run the command.
Paste the pinned version. Prompt text, task notes and recall are routing information, not
evidence — this is the step that most often flips the answer.

**4. Name the strongest alternative and why it loses.** Not a straw version. If you cannot
state a real one, you have not looked.

**5. Decide, and record the mechanism.** A conclusion that depends on people remembering it
will decay. Ask: which check would fail if this were violated? If none, either add one or say
plainly that nothing enforces it.

## Failure signatures

| Signature | What it actually means |
| --- | --- |
| "This should work" | it has not been run |
| "It's probably fine" | the falsifier was never named |
| No alternative considered | the first idea is being defended, not tested |
| The evidence is a memory or a summary | it has not been checked against the repo |
| The rule lives only in prose | it is unenforced, whatever the document says |
| A gate went green immediately | check the gate ran and inspected something, not just that it exited 0 |

## Reporting

Give the conclusion first, then the evidence, then what remains unverified — explicitly.
"Verified X by running Y; Z is untested because W" is a usable answer. "Done" is not, unless
everything was actually checked.

When a prior claim of yours turns out wrong, correct it in one sentence and continue. Do not
re-litigate it.
