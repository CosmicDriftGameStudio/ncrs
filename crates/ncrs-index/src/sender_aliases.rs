use rusqlite::{params, Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{SenderAlias, SenderAliasId, SenderAliasKind, SenderAliasState, SenderAliases};
use crate::names::{name_id, NameTable};

impl Index {
    /// Offers `value` as another name of `sender`. Returns whether a new offer
    /// was stored: not when the value is empty, is the sender's own name, was
    /// offered to this sender before (in any state, so a discarded variant is not
    /// offered to the same sender again), or is offered or confirmed for another
    /// sender. A value another sender discarded may still be offered here.
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
             ON CONFLICT DO NOTHING",
            params![
                sender_id,
                kind.as_str(),
                normalised,
                SenderAliasState::Offered.as_str()
            ],
        )?;
        Ok(inserted > 0)
    }

    /// Fails with `SenderAliasTaken` when another sender holds the same value
    /// as an offer or a confirmed alias.
    pub fn confirm_sender_alias(&self, id: SenderAliasId) -> Result<(), IndexError> {
        if let Some(sender) = other_holder_of_value(self.connection(), id)? {
            return Err(IndexError::SenderAliasTaken { sender });
        }
        self.set_sender_alias_state(id, SenderAliasState::Confirmed)
    }

    /// Confirms `id` and discards the offers or aliases of other senders with
    /// the same value, in one transaction.
    pub fn reassign_sender_alias(&mut self, id: SenderAliasId) -> Result<(), IndexError> {
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "UPDATE sender_alias SET state = ?2
             WHERE id <> ?1 AND state <> ?2
               AND (kind, value) = (SELECT kind, value FROM sender_alias WHERE id = ?1)",
            params![id.get(), SenderAliasState::Discarded.as_str()],
        )?;
        let changed = tx.execute(
            "UPDATE sender_alias SET state = ?2 WHERE id = ?1",
            params![id.get(), SenderAliasState::Confirmed.as_str()],
        )?;
        if changed == 0 {
            return Err(IndexError::SenderAliasNotFound(id));
        }
        tx.commit()?;
        Ok(())
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

fn other_holder_of_value(
    connection: &Connection,
    id: SenderAliasId,
) -> Result<Option<String>, IndexError> {
    Ok(connection
        .query_row(
            "SELECT s.name FROM sender_alias a
             JOIN sender_alias o ON o.kind = a.kind AND o.value = a.value
               AND o.id <> a.id AND o.state <> ?2
             JOIN sender s ON s.id = o.sender_id
             WHERE a.id = ?1",
            params![id.get(), SenderAliasState::Discarded.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
    fn a_discarded_offer_blocks_only_the_pair_of_sender_and_value() {
        let (_dir, index) = encrypted_index();
        let offer = |sender: &str| {
            index
                .offer_sender_alias(sender, SenderAliasKind::Domain, "example.com")
                .unwrap()
        };
        let discard_of = |sender: &str| {
            index
                .sender_aliases()
                .unwrap()
                .into_iter()
                .find(|alias| alias.sender == sender)
                .unwrap()
                .id
        };
        assert!(offer("A GmbH"));
        index.discard_sender_alias(discard_of("A GmbH")).unwrap();

        assert!(offer("B GmbH"));
        index.discard_sender_alias(discard_of("B GmbH")).unwrap();

        assert!(!offer("A GmbH"));
        assert!(!offer("B GmbH"));
        assert!(offer("C GmbH"));
        let aliases = index.sender_aliases().unwrap();
        assert_eq!(aliases.len(), 3);
        assert_eq!(
            aliases.iter().map(|alias| alias.state).collect::<Vec<_>>(),
            [
                SenderAliasState::Discarded,
                SenderAliasState::Discarded,
                SenderAliasState::Offered
            ]
        );
        assert!(index
            .confirmed_sender_aliases()
            .unwrap()
            .sender_for(SenderAliasKind::Domain, "example.com")
            .is_none());
        assert_eq!(index.sender_names().unwrap().len(), 3);
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

    /// A holds the value discarded, B offers it, then A's row is revived.
    fn contested_value(index: &Index) -> (SenderAliasId, SenderAliasId) {
        let id_of = |sender: &str| {
            index
                .sender_aliases()
                .unwrap()
                .into_iter()
                .find(|alias| alias.sender == sender)
                .unwrap()
                .id
        };
        index
            .offer_sender_alias("A GmbH", SenderAliasKind::Domain, "example.com")
            .unwrap();
        let first = id_of("A GmbH");
        index.discard_sender_alias(first).unwrap();
        index
            .offer_sender_alias("B GmbH", SenderAliasKind::Domain, "example.com")
            .unwrap();
        (first, id_of("B GmbH"))
    }

    #[test]
    fn confirming_a_value_another_sender_holds_is_refused() {
        let (_dir, index) = encrypted_index();
        let (first, _) = contested_value(&index);
        let before = index.sender_aliases().unwrap();

        let err = index.confirm_sender_alias(first).unwrap_err();

        assert!(matches!(err, IndexError::SenderAliasTaken { sender } if sender == "B GmbH"));
        assert_eq!(index.sender_aliases().unwrap(), before);
    }

    #[test]
    fn reassigning_moves_the_value_to_the_new_sender() {
        let (_dir, mut index) = encrypted_index();
        let (first, second) = contested_value(&index);

        index.reassign_sender_alias(first).unwrap();

        let states: Vec<_> = index
            .sender_aliases()
            .unwrap()
            .into_iter()
            .map(|alias| (alias.id, alias.state))
            .collect();
        assert!(states.contains(&(first, SenderAliasState::Confirmed)));
        assert!(states.contains(&(second, SenderAliasState::Discarded)));
        assert_eq!(
            index
                .confirmed_sender_aliases()
                .unwrap()
                .sender_for(SenderAliasKind::Domain, "example.com"),
            Some("A GmbH")
        );
    }

    #[test]
    fn reassigning_an_unknown_alias_changes_nothing() {
        let (_dir, mut index) = encrypted_index();
        contested_value(&index);
        let before = index.sender_aliases().unwrap();

        let err = index
            .reassign_sender_alias(SenderAliasId::new(99))
            .unwrap_err();

        assert!(matches!(err, IndexError::SenderAliasNotFound(_)));
        assert_eq!(index.sender_aliases().unwrap(), before);
    }
}
