//! Test-only destination admission, before a credential read or fixture operation.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::model::{DatasetName, ModelName, ProjectName};
use sutura_exec_bigquery::transport::{DatasetId, ProjectId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    WritableDatasets,
    ReadOnlyProjects,
}

pub(crate) struct RawVenue<'a> {
    pub(crate) billing: &'a str,
    pub(crate) default: (&'a str, &'a str),
    pub(crate) protected: (&'a str, &'a str),
    pub(crate) absent: (&'a str, &'a str),
}

pub(crate) struct RawPlacement<'a> {
    pub(crate) model: &'a str,
    pub(crate) address: (&'a str, &'a str),
}

// No resource-bearing Debug: refusals print a closed class, not the supplied value.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Address {
    pub(crate) project: ProjectId,
    pub(crate) dataset: DatasetId,
}

impl Address {
    #[expect(
        clippy::map_err_ignore,
        reason = "resource-bearing parse details are deliberately reduced to a closed fixture admission class"
    )]
    fn parse((project, dataset): (&str, &str)) -> Result<Self, InvalidVenue> {
        // Both the transport and the compiler must accept the destination before any I/O.
        ProjectName::parse(project).map_err(|_| InvalidVenue::Address)?;
        DatasetName::parse(dataset).map_err(|_| InvalidVenue::Address)?;
        Ok(Self {
            project: ProjectId::parse(project).map_err(|_| InvalidVenue::Address)?,
            dataset: DatasetId::parse(dataset).map_err(|_| InvalidVenue::Address)?,
        })
    }
}

pub(crate) struct Layout {
    pub(crate) default: Address,
    pub(crate) absent: Address,
    pub(crate) models: BTreeMap<ModelName, Address>,
    pub(crate) mode: Mode,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InvalidVenue {
    #[error("a complete, parseable project and dataset are required for every address")]
    Address,
    #[error("the default project's identity must equal the declared billing project")]
    Billing,
    #[error("the protected row-policy dataset is not a fixture destination")]
    Protected,
    #[error("each catalog model needs exactly one destination, with no additional models")]
    Mapping,
    #[error("writable fixtures need distinct datasets within the billing project")]
    WritableScope,
    #[error("the read-only mirror needs a destination outside the billing project")]
    ReadOnlyScope,
    #[error("the negative address must be separate from every admitted or protected dataset")]
    NegativeScope,
}

impl Layout {
    #[expect(
        clippy::map_err_ignore,
        reason = "fixture admission reports the closed class, never supplied resource or model text"
    )]
    fn parse(
        raw: &RawVenue<'_>,
        placements: &[RawPlacement<'_>],
        expected: &BTreeSet<ModelName>,
        mode: Mode,
    ) -> Result<Self, InvalidVenue> {
        let billing = ProjectId::parse(raw.billing).map_err(|_| InvalidVenue::Address)?;
        let default = Address::parse(raw.default)?;
        // Mandatory even for read-only execution: omission is not evidence of separation.
        let protected = Address::parse(raw.protected)?;
        let absent = Address::parse(raw.absent)?;
        if default.project != billing {
            return Err(InvalidVenue::Billing);
        }
        if default == protected {
            return Err(InvalidVenue::Protected);
        }
        let mut models = BTreeMap::new();
        for placement in placements {
            let model = ModelName::parse(placement.model).map_err(|_| InvalidVenue::Mapping)?;
            let address = Address::parse(placement.address)?;
            if address == protected {
                return Err(InvalidVenue::Protected);
            }
            if models.insert(model, address).is_some() {
                return Err(InvalidVenue::Mapping);
            }
        }
        if models.keys().collect::<BTreeSet<_>>() != expected.iter().collect() {
            return Err(InvalidVenue::Mapping);
        }
        let destinations: BTreeSet<_> = models.values().collect();
        if absent.project != billing || absent == default || absent == protected || destinations.contains(&absent) {
            return Err(InvalidVenue::NegativeScope);
        }
        match mode {
            Mode::WritableDatasets if destinations.len() < 2 || destinations.iter().any(|address| address.project != billing) => {
                return Err(InvalidVenue::WritableScope);
            }
            Mode::ReadOnlyProjects if destinations.iter().all(|address| address.project == billing) => {
                return Err(InvalidVenue::ReadOnlyScope);
            }
            Mode::WritableDatasets | Mode::ReadOnlyProjects => {}
        }
        Ok(Self {
            default,
            absent,
            models,
            mode,
        })
    }
}

#[derive(Debug)]
pub(crate) enum Operation {
    Load,
    Query,
    Drop,
}

#[derive(thiserror::Error)]
pub(crate) enum Failed {
    #[error(transparent)]
    Configuration(#[from] InvalidVenue),
    #[error("the fixture credential does not match the admitted default destination")]
    Credential,
    #[error("fixture setup failed")]
    Load,
    #[error("the compiled fixture query did not return the required rows")]
    Query,
    #[error("the negative control did not receive the endpoint's not-found refusal")]
    Negative,
    #[error("fixture {at:?} operation failed; resource-bearing cause retained, not printed")]
    Operation {
        at: Operation,
        #[source]
        cause: Box<dyn core::error::Error>,
    },
}

impl Failed {
    pub(crate) fn during(at: Operation, cause: impl core::error::Error + 'static) -> Self {
        Self::Operation {
            at,
            cause: Box::new(cause),
        }
    }
}

impl core::fmt::Debug for Failed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // An explicit source-chain traversal can disclose details; ordinary test Debug cannot.
        core::fmt::Display::fmt(self, f)
    }
}

/// The first implementation is the live fixture; the fake counts effects at this same boundary.
pub(crate) trait FixturePort {
    fn open(&mut self, layout: &Layout) -> Result<(), Failed>;
    fn load(&mut self, layout: &Layout) -> Result<(), Failed>;
    fn compare(&mut self, layout: &Layout) -> Result<(), Failed>;
    fn drop_tables(&mut self, layout: &Layout) -> Result<(), Failed>;
}

pub(crate) fn run(
    raw: &RawVenue<'_>,
    placements: &[RawPlacement<'_>],
    expected: &BTreeSet<ModelName>,
    mode: Mode,
    port: &mut impl FixturePort,
) -> Result<(), Failed> {
    let layout = Layout::parse(raw, placements, expected, mode)?;
    port.open(&layout)?;
    if layout.mode == Mode::ReadOnlyProjects {
        return port.compare(&layout);
    }
    // Even a partial load or a refused comparison attempts cleanup of only this run's names.
    // The loader's 24-hour expiration remains the bound for abort/cancellation.
    let result = port.load(&layout).and_then(|()| port.compare(&layout));
    let cleanup = port.drop_tables(&layout);
    result.and(cleanup)
}
