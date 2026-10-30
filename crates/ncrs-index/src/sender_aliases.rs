use rusqlite::params;

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{SenderAlias, SenderAliasId, SenderAliasKind, SenderAliasState, SenderAliases};
use crate::names::{name_id, NameTable};

impl Index {
    /// Offers `value` as another name of `sender`. Returns whether a new offer
    /// was stored: not when the value is empty, is the sender's own name, or is
    /// known already, in any state, so a discarded variant is not offered again.
    pub fn offer_sender_alias(
        &self,
        sender: &str,
        kind: SenderAliasKind,
        value: &str,
    ) -> Result<bool, IndexError> {
        let normalised = kind.normalise(value);
        if normalised.is_empty()
            || (kind == SenderAliasKind::Name
                && normalised == SenderAliasKind::Name.normalise(sender))
        {
            return Ok(false);
        }
        let sender_id = name_id(self.connection(), NameTable::Sender, sender)?;
        let inserted = self.connection().execute(
            "INSERT INTO sender_alias (sender_id, kind, value, state) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (kind, value) DO NOTHING",
            params![
                sender_id,
                kind.as_str(),
                normalised,
                SenderAliasState::Offered.as_str()
            ],
        )?;
        Ok(inserted > 0)
    }

    pub fn confirm_sender_alias(&self, id: SenderAliasId) -> Result<(), IndexError> {
        self.set_sender_alias_state(id, SenderAliasState::Confirmed)
    }

    pub fn discard_sender_alias(&self, id: SenderAliasId) -> Result<(), IndexError> {
        self.set_sender_alias_state(id, SenderAliasState::Discarded)
    }

    fn set_sender_alias_state(
        &self,
        id: SenderAliasId,
        state: SenderAliasState,
    ) -> Result<(), IndexError> {
        let changed = self.connection().execute(
            "UPDATE sender_alias SET state = ?2 WHERE id = ?1",
            params![id.get(), state.as_str()],
        )?;
        if changed == 0 {
            return Err(IndexError::SenderAliasNotFound(id));
        }
        Ok(())
    }

    /// Ordered by sender name, kind, value.
    pub fn sender_aliases(&self) -> Result<Vec<SenderAlias>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT a.id, s.name, a.kind, a.value, a.state
             FROM sender_alias a JOIN sender s ON s.id = a.sender_id
             ORDER BY s.name, a.kind, a.value",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    SenderAliasId::new(row.get(0)?),
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, sender, kind, value, state)| {
                Ok(SenderAlias {
                    id,
                    sender,
                    kind: SenderAliasKind::parse(&kind)?,
                    value,
                    state: SenderAliasState::parse(&state)?,
                })
            })
            .collect()
    }

    pub fn confirmed_sender_aliases(&self) -> Result<SenderAliases, IndexError> {
        let all = self.sender_aliases()?;
        let confirmed = all
            .iter()
            .filter(|alias| alias.state == SenderAliasState::Confirmed)
            .map(|alias| (alias.kind, alias.value.as_str(), alias.sender.as_str()));
        Ok(SenderAliases::from_confirmed(confirmed))
    }

    /// The names of all senders, in the spelling first seen.
    pub fn sender_names(&self) -> Result<Vec<String>, IndexError> {
        let mut statement = self
            .connection()
            .prepare("SELECT name FROM sender ORDER BY name_key")?;
        let names = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::encrypted_index;

    fn only_alias(index: &Index) -> SenderAlias {
        let mut all = index.sender_aliases().unwrap();
        assert_eq!(all.len(), 1);
        all.remove(0)
    }

    #[test]
    fn an_offer_is_confirmed_and_then_resolves_the_sender() {
        let (_dir, index) = encrypted_index();
        assert!(index
            .offer_sender_alias(
                "Nordlicht Energie GmbH",
                SenderAliasKind::Iban,
                "DE89 3704 0044 0532 0130 00"
            )
            .unwrap());
        let offered = only_alias(&index);
        assert_eq!(offered.state, SenderAliasState::Offered);
        assert_eq!(offered.value, "DE89370400440532013000");
        let before = index.confirmed_sender_aliases().unwrap();
        assert_eq!(
            before.sender_for(SenderAliasKind::Iban, "DE89370400440532013000"),
            None
        );

        index.confirm_sender_alias(offered.id).unwrap();

        let aliases = index.confirmed_sender_aliases().unwrap();
        assert_eq!(
            aliases.sender_for(SenderAliasKind::Iban, "de89 3704 0044 0532 0130 00"),
            Some("Nordlicht Energie GmbH")
        );
        assert_eq!(index.sender_names().unwrap(), ["Nordlicht Energie GmbH"]);
    }

    #[test]
    fn a_discarded_variant_is_not_offered_again() {
        let (_dir, index) = encrypted_index();
        index
            .offer_sender_alias("Telekom Deutschland GmbH", SenderAliasKind::Name, "Telekom")
            .unwrap();
        let id = only_alias(&index).id;
        index.discard_sender_alias(id).unwrap();

        let again = index
            .offer_sender_alias(
                "Telekom Deutschland GmbH",
                SenderAliasKind::Name,
                "  TELEKOM ",
            )
            .unwrap();

        assert!(!again);
        assert_eq!(only_alias(&index).state, SenderAliasState::Discarded);
        assert!(
            index
                .confirmed_sender_aliases()
                .unwrap()
                .canonical_sender("Telekom")
                == "Telekom"
        );
    }

    #[test]
    fn the_same_value_cannot_belong_to_two_senders() {
        let (_dir, index) = encrypted_index();
        assert!(index
            .offer_sender_alias("A GmbH", SenderAliasKind::Domain, "www.example.com")
            .unwrap());

        let second = index
            .offer_sender_alias("B GmbH", SenderAliasKind::Domain, "Example.com")
            .unwrap();

        assert!(!second);
        assert_eq!(only_alias(&index).sender, "A GmbH");
    }

    #[test]
    fn iban_and_tax_id_spellings_are_one_alias() {
        let (_dir, index) = encrypted_index();
        assert!(index
            .offer_sender_alias(
                "A GmbH",
                SenderAliasKind::Iban,
                "DE89 3704 0044 0532 0130 00"
            )
            .unwrap());
        assert!(!index
            .offer_sender_alias("A GmbH", SenderAliasKind::Iban, "de89370400440532013000")
            .unwrap());
        assert!(index
            .offer_sender_alias("A GmbH", SenderAliasKind::TaxId, "12/345/67890")
            .unwrap());
        assert!(!index
            .offer_sender_alias("A GmbH", SenderAliasKind::TaxId, "12 345 67890")
            .unwrap());
        assert_eq!(index.sender_aliases().unwrap().len(), 2);
    }

    #[test]
    fn empty_values_and_the_own_name_are_not_offered() {
        let (_dir, index) = encrypted_index();
        assert!(!index
            .offer_sender_alias("A GmbH", SenderAliasKind::Domain, "  ")
            .unwrap());
        assert!(!index
            .offer_sender_alias("A  GmbH", SenderAliasKind::Name, "a gmbh")
            .unwrap());
        assert!(index.sender_aliases().unwrap().is_empty());
    }

    #[test]
    fn a_confirmed_name_alias_maps_the_name_to_its_sender() {
        let (_dir, index) = encrypted_index();
        index
            .offer_sender_alias("Telekom", SenderAliasKind::Name, "Telekom Deutschland GmbH")
            .unwrap();
        index.confirm_sender_alias(only_alias(&index).id).unwrap();

        let aliases = index.confirmed_sender_aliases().unwrap();

        assert_eq!(
            aliases.canonical_sender("telekom  deutschland gmbh"),
            "Telekom"
        );
        assert_eq!(aliases.canonical_sender("Other"), "Other");
    }

    #[test]
    fn an_unknown_alias_is_not_found() {
        let (_dir, index) = encrypted_index();
        let err = index
            .confirm_sender_alias(SenderAliasId::new(9))
            .unwrap_err();
        assert!(matches!(err, IndexError::SenderAliasNotFound(id) if id == SenderAliasId::new(9)));
    }
}
