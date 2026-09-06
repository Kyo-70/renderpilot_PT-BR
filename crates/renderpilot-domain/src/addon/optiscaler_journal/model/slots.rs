/// Current occupation of the private artifact slots for one operation.
///
/// Action states carry immutable historical proofs.  These slots contain only
/// the latest exact durable occupation of custody, staging, and discard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateArtifactSlots {
    custody: DurableObservation,
    stage: DurableObservation,
    discard: DurableObservation,
}

impl<'de> Deserialize<'de> for PrivateArtifactSlots {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            custody: DurableObservation,
            stage: DurableObservation,
            discard: DurableObservation,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.custody, wire.stage, wire.discard).map_err(serde::de::Error::custom)
    }
}

impl PrivateArtifactSlots {
    /// Creates an empty or occupied slot set and rejects uncertain observations.
    pub fn new(
        custody: DurableObservation,
        stage: DurableObservation,
        discard: DurableObservation,
    ) -> Result<Self, OptiScalerJournalError> {
        let slots = Self {
            custody,
            stage,
            discard,
        };
        slots.validate()?;
        Ok(slots)
    }

    /// Returns the current custody slot occupation.
    #[must_use]
    pub fn custody(&self) -> &DurableObservation {
        &self.custody
    }

    /// Returns the current stage slot occupation.
    #[must_use]
    pub fn stage(&self) -> &DurableObservation {
        &self.stage
    }

    /// Returns the current discard slot occupation.
    #[must_use]
    pub fn discard(&self) -> &DurableObservation {
        &self.discard
    }

    /// Returns a mutable custody slot for the durable CAS result.
    pub fn stage_mut(&mut self) -> &mut DurableObservation {
        &mut self.stage
    }

    /// Returns a mutable stage slot for the durable CAS result.
    pub fn custody_mut(&mut self) -> &mut DurableObservation {
        &mut self.custody
    }

    /// Returns a mutable discard slot for the durable CAS result.
    pub fn discard_mut(&mut self) -> &mut DurableObservation {
        &mut self.discard
    }

    /// Validates that every current slot is absent or exact.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        let mut identities = HashSet::new();
        for observation in [&self.custody, &self.stage, &self.discard] {
            observation.validate()?;
            if !observation.is_exact() {
                return Err(OptiScalerJournalError::Invalid(
                    "private artifact slot observation must be exact",
                ));
            }
            if let Some(identity) = observation.identity()
                && !identities.insert(identity)
            {
                return Err(OptiScalerJournalError::Invalid(
                    "private artifact slots must not share a native identity",
                ));
            }
        }
        Ok(())
    }
}
