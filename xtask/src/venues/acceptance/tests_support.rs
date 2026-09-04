//! The workflow this module's own suite perturbs, and the perturbations more than one test makes.
//!
//! In its own file for the reason [`super::shape`] was split out: the parent reached the 1000-line
//! gate, and the gate's answer is to split rather than to shorten what is documented. **The harness
//! moves and every assertion stays**, which is the half the first split got backwards - a file that
//! adds no `#[test]` is one the causality gate reverts, so moving an assertion out of the parent
//! would orphan it and read as green against base.

/// The one line that makes the fixture keyed, so every test that removes it removes the same thing.
pub(super) const KEY_ENV: &str = "          SUTURA_BQ_KEY: ${{ secrets.a_key }}\n";

/// The job's own condition, built from the constant the gate reads - so a change to the rule cannot
/// leave a fixture perturbing a string nothing looks for any more.
pub(super) fn condition() -> String {
    format!("if: github.event_name == 'push' || {}", super::FORK_RULE)
}

/// The one line that stores the key, which most perturbations here add a line beside.
const WRITE: &str = "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n";

/// The job with `added` on the line after the write.
///
/// **The write STAYS**, so nothing else in the job is disturbed and only the check under test can
/// produce a failure - which is what makes an exact `found.len()` an assertion rather than a hope.
pub(super) fn beside_the_write(added: &str) -> String {
    CI.replace(WRITE, &format!("{WRITE}          {added}\n"))
}

/// The job with the write itself replaced by `instead`, at the same indentation.
pub(super) fn instead_of_the_write(instead: &str) -> String {
    CI.replace(WRITE, &format!("          {instead}\n"))
}

/// A step written with no `name:`, which is the form the shell scan could not see.
pub(super) fn with_nameless_step(body: &str) -> String {
    CI.replace(
        "      - name: Remove the credential\n",
        &format!("      - run: {body}\n\n      - name: Remove the credential\n"),
    )
}

/// A Google token exchange in the job, which is the signal `docs/adr/0017` names.
pub(super) fn with_google_exchange(ci: &str) -> String {
    ci.replace(
        "      - name: Acceptance leg\n",
        "      - uses: google-github-actions/auth@v2\n      - name: Acceptance leg\n",
    )
}

/// `id-token: write` on the job - the permission the record proves is NOT the signal.
///
/// `needs: [ci]` appears twice below, so this grants it to `cross` as well. Harmless, because
/// [`super::shape::job`] reads one block, and written here rather than rediscovered per test.
pub(super) fn with_id_token(ci: &str) -> String {
    ci.replace(
        "    needs: [ci]\n",
        "    needs: [ci]\n    permissions:\n      id-token: write\n",
    )
}

/// The real acceptance job, with each property removed in turn.
pub(super) const CI: &str = "\
jobs:
  bigquery-acceptance:
    needs: [ci]
    if: github.event_name == 'push' || github.event.pull_request.head.repo.full_name == github.repository
    environment: bq-test
    steps:
      - name: Place the credential outside the checkout
        env:
          SUTURA_BQ_KEY: ${{ secrets.a_key }}
        run: |
          set -eu
          if [ -z \"${SUTURA_BQ_KEY:-}\" ]; then
            echo \"the environment holds no key\" >&2
            exit 1
          fi
          umask 077
          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"

      # A comment between two steps, mentioning ${{ }} the way this file does.
      - name: Acceptance leg
        env:
          GOOGLE_APPLICATION_CREDENTIALS: ${{ runner.temp }}/bq-key.json
          SUTURA_BQ_DATASET: ${{ vars.SUTURA_BQ_DATASET }}
        run: |
          set -eu
          for name in SUTURA_BQ_DATASET; do
            if [ -z \"$(printenv \"$name\" || true)\" ]; then
              echo \"the environment defines no $name\" >&2
              exit 1
            fi
          done
          nix run .#bigquery-acceptance

      - name: Remove the credential
        if: always()
        run: rm -f \"$RUNNER_TEMP/bq-key.json\"

  cross:
    needs: [ci]
";
