/// The role of an endpoint in an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Endpoint {
    /// A single-path action (write, delete, verify, or directory action).
    Single,
    /// The source side of a relocation.
    Source,
    /// The destination side of a relocation.
    Destination,
}

/// The source of an operation's resolved preimage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Preimage {
    /// The observation captured before the first operation touching a path.
    Initial {
        /// Historical filesystem observation before any mutation started.
        observation: DurableObservation,
        /// Recorded file receipt associated with the path, if previously managed.
        receipt: Option<FileReceipt>,
        /// Durable proof of ownership basis when modifying an owned file.
        owned_basis: Option<FileReceipt>,
    },
    /// The exact postimage produced by an earlier operation in this ordered
    /// program.  The reference is deliberately explicit so repeated paths
    /// cannot be resolved by a path-only first/last lookup.
    PriorPostimage {
        /// Identifier of the earlier operation that produced this postimage.
        operation_id: u32,
        /// Endpoint side (single, source, or destination) of the referenced operation.
        endpoint: Endpoint,
    },
}

impl<'de> Deserialize<'de> for Preimage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Initial {
                observation: DurableObservation,
                receipt: Option<FileReceipt>,
                owned_basis: Option<FileReceipt>,
            },
            PriorPostimage {
                operation_id: u32,
                endpoint: Endpoint,
            },
        }

        let wire = Wire::deserialize(deserializer)?;
        let preimage = match wire {
            Wire::Initial {
                observation,
                receipt,
                owned_basis,
            } => Self::Initial {
                observation,
                receipt,
                owned_basis,
            },
            Wire::PriorPostimage {
                operation_id,
                endpoint,
            } => Self::PriorPostimage {
                operation_id,
                endpoint,
            },
        };
        preimage.validate().map_err(serde::de::Error::custom)?;
        Ok(preimage)
    }
}

impl Preimage {
    /// Validates receipt/observation coupling and reference shape.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        match self {
            Self::Initial {
                observation,
                receipt,
                owned_basis,
            } => {
                observation.validate()?;
                match (observation, receipt, owned_basis) {
                    (DurableObservation::File { identity, digest }, Some(receipt), basis) => {
                        if receipt.identity() != identity || receipt.digest() != digest {
                            return Err(OptiScalerJournalError::Invalid(
                                "initial file receipt does not match observation",
                            ));
                        }
                        if let Some(basis) = basis {
                            // `owned_basis` is the exact authority captured
                            // before an edited owned file was replaced.  Its
                            // bytes may legitimately differ from the current
                            // receipt; identity and ownership must not.
                            if basis.ownership() != FileOwnership::Owned
                                || basis.identity() != identity
                            {
                                return Err(OptiScalerJournalError::Invalid(
                                    "initial owned basis does not match observation",
                                ));
                            }
                        }
                    }
                    (DurableObservation::File { .. }, None, Some(_)) => {
                        return Err(OptiScalerJournalError::Invalid(
                            "initial owned basis requires a file receipt",
                        ));
                    }
                    (DurableObservation::File { .. } | DurableObservation::Absent |
DurableObservation::Directory { .. } | DurableObservation::NonRegular |
DurableObservation::Unreadable, None, None) => {}
                    (_, _, _) => {
                        return Err(OptiScalerJournalError::Invalid(
                            "initial receipt is only valid for an exact file observation",
                        ));
                    }
                }
            }
            Self::PriorPostimage { .. } => {}
        }
        Ok(())
    }
}

/// The expected postimage of an endpoint while a mutation is in progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedAfter {
    /// The operation has not produced its postimage yet.
    Pending,
    /// The exact postimage is durably known.
    Known(DurableObservation),
}

impl ExpectedAfter {
    /// Validates an expected postimage.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        if let Self::Known(observation) = self {
            observation.validate()?;
            if !observation.is_exact() {
                return Err(OptiScalerJournalError::Invalid(
                    "known postimage must be an exact observation",
                ));
            }
        }
        Ok(())
    }
}

/// One path endpoint in the ordered mutation program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationEndpoint {
    endpoint: Endpoint,
    path: String,
    preimage: Preimage,
    expected_after: ExpectedAfter,
}

impl<'de> Deserialize<'de> for OperationEndpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            endpoint: Endpoint,
            path: String,
            preimage: Preimage,
            expected_after: ExpectedAfter,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.endpoint, wire.path, wire.preimage, wire.expected_after)
            .map_err(serde::de::Error::custom)
    }
}

impl OperationEndpoint {
    /// Constructs an endpoint after validating its domain fields.
    pub fn new(
        endpoint: Endpoint,
        path: impl Into<String>,
        preimage: Preimage,
        expected_after: ExpectedAfter,
    ) -> Result<Self, OptiScalerJournalError> {
        let endpoint = Self {
            endpoint,
            path: into_nonempty("operation endpoint path", path.into())?,
            preimage,
            expected_after,
        };
        endpoint.validate()?;
        Ok(endpoint)
    }

    /// Returns the endpoint role.
    #[must_use]
    pub const fn endpoint(&self) -> Endpoint {
        self.endpoint
    }

    /// Returns the domain-neutral path text.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the endpoint preimage reference.
    #[must_use]
    pub fn preimage(&self) -> &Preimage {
        &self.preimage
    }

    /// Returns the expected postimage.
    #[must_use]
    pub fn expected_after(&self) -> &ExpectedAfter {
        &self.expected_after
    }

    /// Updates only the expected postimage; path and preimage identity remain
    /// immutable at the orchestration/storage boundary.
    pub fn set_expected_after(&mut self, expected_after: ExpectedAfter) {
        self.expected_after = expected_after;
    }

    /// Validates endpoint invariants.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        validate_nonempty("operation endpoint path", &self.path)?;
        self.preimage.validate()?;
        if let Preimage::Initial { observation, .. } = &self.preimage {
            observation.validate()?;
            if !observation.is_exact() {
                return Err(OptiScalerJournalError::Invalid(
                    "operation preimage observation must be exact",
                ));
            }
        }
        self.expected_after.validate()
    }
}
