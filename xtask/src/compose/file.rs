//! Gates that read `compose.services.yaml`'s TEXT.
//!
//! The parent module's suite is mostly about the functions above it - what `dev-up` puts on a
//! command line, which services a profile selects. These two read the file itself, because
//! **no other gate in this repository reads a compose file** and a rule about its shape that
//! nothing scans is a rule held by recall.

/// The compose file's text, or `None` where the repo root cannot be found.
fn compose_text() -> Option<String> {
    let root = crate::repo::root()?;
    std::fs::read_to_string(root.join(super::docker::COMPOSE_FILE)).ok()
}

/// One service block in the tier: its name, and the lines belonging to it.
struct Block<'a> {
    /// The two-space key under `services:`, which is the name every other mechanism uses.
    name: &'a str,
    /// Its own lines, so a key is attributed to the service it is nested under.
    body: Vec<&'a str>,
}

impl Block<'_> {
    /// Does this service declare a health probe?
    fn probes(&self) -> bool {
        self.body.iter().any(|line| line.trim() == "healthcheck:")
    }
}

/// Every service block in the tier.
///
/// Bounded to the `services:` mapping: the `x-` blocks at the head and `volumes:` at the foot also
/// have two-space keys, so a scan running to the end of the file would report a named volume as a
/// service with no healthcheck.
fn service_blocks(text: &str) -> Vec<Block<'_>> {
    let mut blocks: Vec<Block<'_>> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if !line.is_empty() && !line.starts_with([' ', '#']) {
            inside = line.trim_end() == "services:";
            continue;
        }
        if !inside {
            continue;
        }
        if let Some(name) = line.strip_prefix("  ")
            && !name.starts_with([' ', '#'])
            && let Some(name) = name.strip_suffix(':')
            && !name.contains(' ')
        {
            blocks.push(Block { name, body: Vec::new() });
        } else if let Some(block) = blocks.last_mut() {
            block.body.push(line);
        }
    }
    blocks
}

#[test]
fn shared_configuration_is_defined_above_the_service_list() {
    // Where an anchor is DEFINED is a review property, not a style one: configuration anchored
    // inside one service block reads as the service that merges it INHERITING from that one. It does
    // not - a merge key is a copy - and the misreading is not hypothetical. The DataHub backend
    // environment was anchored inside the migration job, and a review of the merge key on the
    // service below it concluded that GMS was absent from the file.
    let Some(text) = compose_text() else { return };
    let Some(services_at) = text.lines().position(|line| line.trim_end() == "services:") else {
        panic!("{} declares no `services:` mapping", super::docker::COMPOSE_FILE);
    };

    let mut defined: Vec<(usize, &str)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        for at in line.char_indices().filter(|&(_, c)| c == '&').map(|(at, _)| at) {
            // An anchor DEFINITION is ` &name`. A JDBC URL's `?a=1&b=2` and a probe's `2>&1` are
            // not, and neither has a space before the ampersand - which is what separates them.
            if at > 0
                && bytes[at - 1] == b' '
                && let Some(rest) = line.get(at + 1..)
                && let Some(name) = rest.split_whitespace().next()
                && name.starts_with(|c: char| c.is_ascii_alphanumeric())
            {
                defined.push((index, name));
            }
        }
    }

    // Or a broken scan passes any file. `the_fixture_credential_is_defined_once` holds this one.
    assert!(
        defined.iter().any(|&(_, name)| name == "fixture-password"),
        "this scan found no anchor definition and would pass anything: {defined:?}"
    );
    for (index, name) in &defined {
        assert!(
            *index < services_at,
            "{}:{}: `&{name}` is shared configuration defined inside `services:` - hoist it to an \
             `x-` block above the mapping, the way `x-fixture-credentials` is",
            super::docker::COMPOSE_FILE,
            index + 1
        );
    }
}

#[test]
fn every_service_in_the_tier_declares_a_healthcheck_unless_it_is_a_job() {
    // Provisioning gates on HEALTH - `wait_until_healthy` polls until every expected service
    // reports healthy - and docker treats a container with no probe as ready the moment it is
    // RUNNING. So a service added without one fails nothing and silently weakens the readiness gate
    // for the whole tier. The head of the compose file has stated that rule since the tier existed,
    // which made it a rule held by recall; this is the mechanism it was missing.
    //
    // A JOB is exempt, derived rather than listed: a container another service depends on with
    // `service_completed_successfully` is expected to EXIT, so a probe on it is a contradiction.
    let Some(text) = compose_text() else { return };
    let blocks = service_blocks(&text);
    assert!(
        blocks.len() >= 5,
        "this scan found {} block(s) and would pass anything",
        blocks.len()
    );

    let mut jobs: Vec<&str> = Vec::new();
    let mut awaited: Option<&str> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("      ")
            && !name.starts_with(' ')
            && let Some(name) = name.strip_suffix(':')
        {
            awaited = Some(name);
        }
        if line.trim() == "condition: service_completed_successfully"
            && let Some(name) = awaited
        {
            jobs.push(name);
        }
    }

    for block in &blocks {
        let name = block.name;
        if jobs.contains(&name) {
            assert!(
                !block.probes(),
                "`{name}` is awaited as a completed job and also probes for health"
            );
        } else {
            assert!(
                block.probes(),
                "`{name}` declares no healthcheck, so provisioning treats it as ready as soon as it \
                 is running - give it a probe, or make it a job something waits to complete"
            );
        }
    }
}
